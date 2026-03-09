# Moto Throttle

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Status | Wrenching |
| Last Updated | 2026-03-09 |

## Overview

Rate limiting library for moto services. Provides per-principal request throttling as a tower middleware layer. Primary use case: per-garage rate limiting on ai-proxy to prevent runaway AI spend.

**Key properties:**
- **Library, not a service** — tower middleware that services embed directly
- **In-memory for v1** — single-instance token bucket per principal (no Redis dependency)
- **Per-instance limits** — each service replica maintains independent state. With N replicas, the effective limit is N × configured RPM. This is acceptable for v1 where ai-proxy runs 1-2 replicas.
- **Principal-aware** — rate limits keyed by SPIFFE ID (garage identity from SVID)
- **Tiered limits** — different limits for different principal types and endpoint categories

**What this is NOT:**
- Not a standalone proxy or sidecar (middleware embedded in each service)
- Not distributed rate limiting (v1 is per-instance; distributed via Redis is future)
- Not DDoS protection (that's handled at the network/ingress level)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Service (e.g., ai-proxy)                                        │
│                                                                   │
│  Router                                                          │
│  └── ThrottleLayer (tower middleware)                             │
│      ├── Extracts principal from request (SVID / service token)  │
│      ├── Looks up rate limit tier for principal type + endpoint   │
│      ├── Checks token bucket (in-memory, per principal)          │
│      ├── If allowed → forward request                            │
│      └── If denied → 429 Too Many Requests                      │
│                                                                   │
│  Buckets: in-memory, one per principal, evicted when idle        │
└─────────────────────────────────────────────────────────────────┘
```

## Rate Limit Tiers

| Tier | Principal Type | Default RPM | Burst | Use Case |
|------|---------------|-------------|-------|----------|
| Garage | `garage` | 120 | 20 | AI API calls through ai-proxy |
| Bike | `bike` | 300 | 50 | Deployed service engines (e.g., moto-club calling keybox) |
| Service | `service` | 1000 | 100 | Internal service calls authenticated via service token |
| Unknown | (no valid auth) | 30 | 5 | Unauthenticated / malformed requests |

RPM = requests per minute. Burst = max requests allowed in a single burst before throttling kicks in.

**Note on principal types:** `garage` and `bike` are both SVID-authenticated principals (see keybox.md). A "bike" is a deployed service engine that authenticates via its own SVID — not to be confused with the moto-bike container image spec. `service` refers to calls authenticated via static service token (e.g., moto-club → keybox).

### Per-endpoint overrides

Services can configure different limits for different endpoint categories:

```rust
ThrottleConfig::new()
    .tier(PrincipalType::Garage, 120, 20)           // default for garages
    .override_path("/v1/chat/completions", PrincipalType::Garage, 60, 10)  // tighter for AI completions
    .override_path("/health/", PrincipalType::Garage, 0, 0)  // no limit on health checks
```

A limit of `0` means no rate limiting for that path/principal combination.

## Token Bucket Algorithm

Each principal gets an independent token bucket:

- **Capacity** = burst size (max tokens)
- **Refill rate** = RPM / 60 tokens per second
- **Cost** = 1 token per request
- Tokens refill continuously (not in discrete intervals)
- Bucket starts full

When a request arrives:
1. Calculate tokens accrued since last request
2. Add accrued tokens (capped at capacity)
3. If tokens >= 1, allow request, subtract 1
4. If tokens < 1, deny with 429

### Bucket cleanup

Buckets that haven't been accessed within the configured TTL (default 10 minutes) are evicted to prevent unbounded memory growth. Eviction runs periodically (default every 60 seconds). Both values are configurable via env vars.

## Principal Extraction

The middleware extracts principal identity from the request to determine the rate limit key:

1. Check `Authorization: Bearer {token}` header — attempt to parse as JWT and extract `principal_type` and `principal_id` claims
2. Check `x-api-key: {token}` header (Anthropic passthrough) — same JWT parse attempt
3. If the token is not a valid JWT (fails JSON parse), check if it matches the service token → tier = Service, key = "service-token"
4. If no valid auth found → tier = Unknown, key = client IP (from `X-Forwarded-For` or socket addr)

**JWT parsing:** The middleware parses the JWT payload to read claims but does NOT validate the signature — that's the auth layer's job (runs after throttle). If the JWT payload is malformed (invalid base64, missing required claims), the request falls through to step 3/4.

**Service token detection:** A service token is a static hex string (not a JWT). The middleware distinguishes by attempting JWT parse first — if that fails, it compares against the configured service token value.

## Response Headers

All responses include rate limit headers (both allowed and denied requests):

```
X-RateLimit-Limit: 120
X-RateLimit-Remaining: 85
X-RateLimit-Reset: 1741500060
```

- `X-RateLimit-Limit`: requests per minute for this tier
- `X-RateLimit-Remaining`: approximate requests remaining (based on current bucket tokens)
- `X-RateLimit-Reset`: Unix timestamp when bucket will be full again

### 429 Response

When rate limited:

```
HTTP/1.1 429 Too Many Requests
Content-Type: application/json
Retry-After: 3
X-RateLimit-Limit: 120
X-RateLimit-Remaining: 0
X-RateLimit-Reset: 1741500060

{"error": {"message": "rate limit exceeded", "type": "rate_limit_error"}}
```

Uses the OpenAI error format for SDK compatibility (consistent with ai-proxy error responses).

`Retry-After` is in seconds — calculated as the time until the bucket has at least one token: `ceil(tokens_needed / refill_rate_per_second)`.

## Integration

### ai-proxy (primary consumer)

```rust
use moto_throttle::{ThrottleLayer, ThrottleConfig, PrincipalType};

let throttle = ThrottleConfig::new()
    .tier(PrincipalType::Garage, 120, 20)
    .tier(PrincipalType::Service, 1000, 100)
    .override_path("/health/", PrincipalType::Garage, 0, 0)
    .build();

let app = Router::new()
    .merge(proxy_routes)
    .layer(ThrottleLayer::new(throttle));
```

The throttle layer sits before the auth layer in the middleware stack. This means unauthenticated floods are rate-limited before they reach auth validation.

### Other services (future)

moto-club and keybox can add throttling the same way. Each service configures its own tiers and overrides.

## Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `MOTO_THROTTLE_GARAGE_RPM` | `120` | Requests per minute for garages |
| `MOTO_THROTTLE_GARAGE_BURST` | `20` | Burst size for garages |
| `MOTO_THROTTLE_SERVICE_RPM` | `1000` | RPM for service principals |
| `MOTO_THROTTLE_SERVICE_BURST` | `100` | Burst for service principals |
| `MOTO_THROTTLE_UNKNOWN_RPM` | `30` | RPM for unauthenticated requests |
| `MOTO_THROTTLE_UNKNOWN_BURST` | `5` | Burst for unauthenticated requests |
| `MOTO_THROTTLE_CLEANUP_INTERVAL_SECS` | `60` | How often to evict expired buckets |
| `MOTO_THROTTLE_BUCKET_TTL_SECS` | `600` | Evict buckets not accessed in this time |

Services can also configure these programmatically via `ThrottleConfig`. Env vars override programmatic defaults.

**Note:** The middleware needs the service token value to distinguish service tokens from JWTs. This is read from the host service's existing `MOTO_KEYBOX_SERVICE_TOKEN` or `MOTO_KEYBOX_SERVICE_TOKEN_FILE` env var — no additional secret configuration needed.

## Observability

**Logging (on 429 only):**

```
level=warn principal_id=garage-abc123 principal_type=garage path=/v1/chat/completions rpm_limit=120 retry_after_secs=3 msg="rate limited"
```

**Metrics (future):**
- `moto_throttle_requests_total{principal_type, decision}` — counter (allowed/denied)
- `moto_throttle_bucket_count` — gauge of active buckets

## Deferred Items

- **Distributed rate limiting (Redis)** — needed when services run multiple replicas and limits must be shared across instances
- **Per-garage configurable limits** — admin sets custom limits per garage (e.g., premium garages get higher RPM)
- **Cost-based limiting** — rate limit by estimated token cost, not just request count
- **Metrics** — Prometheus counters for rate limit decisions

## References

- [ai-proxy.md](ai-proxy.md) — Primary consumer of throttle middleware
- [keybox.md](keybox.md) — Defines SVID principal types (garage, bike, service) used for tier selection. Future consumer.
- [moto-club.md](moto-club.md) — Future consumer

## Changelog

### v0.3 (2026-03-09)
- Move per-instance limits caveat from Deferred Items to Overview (key architectural property, not a deferral).
- Remove Crate Structure section (internal file layout is implementation detail).
- Remove HashMap from architecture diagram (implementation detail).
- Clarify "Bike" principal type: deployed service engines authenticated via SVID, distinct from moto-bike container spec.
- Add JWT parsing section: explain how malformed JWTs are handled, how service tokens are distinguished from JWTs.
- Add Retry-After calculation formula.
- Clarify service token detection: middleware reads from host service's existing `MOTO_KEYBOX_SERVICE_TOKEN` env var.
- Note that throttle layer sits before auth layer in middleware stack.

### v0.2 (2026-03-09)
- Full spec. Library (not service), tower middleware, token bucket, per-principal rate limiting.
- Primary use case: ai-proxy per-garage rate limiting.
- In-memory for v1, distributed Redis deferred.
- Rate limit tiers: garage (120 RPM), bike (300), service (1000), unknown (30).
- Per-endpoint overrides, bucket cleanup, response headers.

### v0.1 (2026-02-04)
- Initial spec (bare frame)
