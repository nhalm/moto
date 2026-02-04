# Moto Throttle

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Bare Frame |
| Last Updated | 2026-02-04 |

## Changelog

### v0.1 (2026-02-04)
- Initial spec (bare frame)

## Overview

Rate limiting middleware for moto services. Provides distributed rate limiting across multiple service instances using Redis as a backing store. Applies to moto-keybox, moto-club, and other HTTP APIs.

## Specification

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        moto-throttle                            │
│                                                                 │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  │
│  │  Middleware     │  │  Rate Limiter   │  │  Redis Backend  │  │
│  │  (tower layer)  │  │  (token bucket) │  │  (distributed)  │  │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### Rate Limit Tiers

To be defined. Consider:

| Tier | Requests/min | Use Case |
|------|-------------|----------|
| Anonymous | 60 | Unauthenticated requests |
| Authenticated | 300 | SVID-authenticated requests |
| Service | 1000 | Service-to-service (moto-club) |

### Key Design

Rate limit keys should include:
- Principal identity (SPIFFE ID or IP for anonymous)
- Endpoint category (auth, secrets, admin)

### Response Headers

Standard rate limit headers:
```
X-RateLimit-Limit: 300
X-RateLimit-Remaining: 299
X-RateLimit-Reset: 1706000000
Retry-After: 60  (when limited)
```

### Error Response

When rate limited, return:
```
HTTP/1.1 429 Too Many Requests
Content-Type: application/json

{
  "error": "RATE_LIMITED",
  "message": "Rate limit exceeded. Retry after 60 seconds.",
  "retry_after": 60
}
```

### Configuration

```bash
# Redis connection for distributed state
MOTO_THROTTLE_REDIS_URL="redis://localhost:6379"

# Default limits (can be overridden per-endpoint)
MOTO_THROTTLE_DEFAULT_RPM="300"
MOTO_THROTTLE_BURST_SIZE="50"
```

### Integration

Services integrate via tower middleware layer:

```rust
// Example integration (to be implemented)
let app = Router::new()
    .merge(api_routes())
    .layer(ThrottleLayer::new(throttle_config));
```

## Notes

- Use token bucket algorithm for smooth rate limiting
- Support per-principal and per-endpoint limits
- Graceful degradation if Redis is unavailable (allow requests, log warning)
- Consider sliding window for fairer distribution

## References

- Tower middleware: https://docs.rs/tower
- Redis rate limiting patterns: https://redis.io/commands/incr#pattern-rate-limiter
