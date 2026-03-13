# AI Proxy

## The Problem

AI development tools like Claude Code need to call AI providers (Anthropic, OpenAI, Google Gemini) to function. But garages run arbitrary, potentially untrusted code—including AI agents with full autonomy inside their containers.

**The security challenge:** If a garage has direct access to real API keys, a compromised agent could:
- Exfiltrate the keys
- Use them outside the garage
- Rack up unlimited charges
- Access other projects or organizations tied to those keys

**The solution:** Garages never see real API keys. Instead, they authenticate using their SPIFFE SVID (a cryptographically-signed, short-lived JWT), and the AI proxy injects real credentials on their behalf.

## How It Works

```
┌─────────────────────────────────────────────────────────────┐
│  Garage Pod                                                  │
│                                                               │
│  Claude Code sends:                                          │
│    POST http://ai-proxy.moto-system:8080/passthrough/        │
│         anthropic/v1/messages                                │
│    x-api-key: {garage-svid-jwt}                              │
│    {...request body}                                         │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      v
┌─────────────────────────────────────────────────────────────┐
│  ai-proxy (moto-system namespace)                            │
│                                                               │
│  1. Validates garage SVID (15-min TTL, pod-bound)            │
│  2. Fetches real API key from keybox                         │
│  3. Replaces garage token with real key                      │
│  4. Forwards to provider                                     │
└─────────────────────┬───────────────────────────────────────┘
                      │
                      v
              api.anthropic.com
```

The garage's environment variables point to the proxy:

```bash
# Claude Code (Anthropic SDK)
ANTHROPIC_BASE_URL=http://ai-proxy.moto-system:8080/passthrough/anthropic
ANTHROPIC_API_KEY={garage-svid-jwt}

# OpenAI-compatible tools (litellm, langchain, etc.)
OPENAI_BASE_URL=http://ai-proxy.moto-system:8080/v1
OPENAI_API_KEY={garage-svid-jwt}
```

The garage entrypoint automatically reads the SVID from `/var/run/secrets/svid/svid.jwt` and sets these environment variables. From the AI tool's perspective, it's using a normal API key—it just happens to be a time-limited, cryptographically-verified identity token.

## Passthrough vs Unified

The AI proxy supports two access modes:

### Passthrough Mode (Primary)

**Use case:** Tools that use a provider's native SDK (e.g., Claude Code with Anthropic SDK).

**How it works:** The proxy forwards requests directly to the provider with zero translation. The garage sends provider-native requests, and the proxy only injects credentials.

**Routes:**
- `/passthrough/anthropic/` → `https://api.anthropic.com/`
- `/passthrough/openai/` → `https://api.openai.com/`
- `/passthrough/gemini/` → `https://generativelanguage.googleapis.com/`

**Example:** Claude Code sends an Anthropic Messages API request. The proxy strips the SVID token, injects the real `x-api-key`, and forwards to `api.anthropic.com`. Responses stream back unchanged.

**Path allowlist:** Only inference endpoints are allowed (e.g., `/v1/messages`, `/v1/chat/completions`). Admin, billing, and account management endpoints return `403 Forbidden`.

### Unified Endpoint (Multi-Provider)

**Use case:** Tools that expect the OpenAI-compatible API format (litellm, langchain ChatOpenAI, openai SDK).

**How it works:** The proxy auto-routes by model name and translates formats as needed.

**Routes:**
- `/v1/chat/completions` — auto-routes based on model prefix
- `/v1/models` — returns merged model list from all configured providers

**Model routing:**
- `claude-*` → Anthropic (with OpenAI ↔ Anthropic translation)
- `gpt-*`, `o1-*`, `o3-*` → OpenAI (no translation, native format)
- `gemini-*` → Google Gemini (via OpenAI-compatible mode)

**Example:** A tool using the OpenAI SDK sends `{"model": "claude-sonnet-4-20250514", ...}` to `/v1/chat/completions`. The proxy detects the `claude-*` prefix, translates the request from OpenAI format to Anthropic Messages API format, forwards to Anthropic, and translates the response back to OpenAI format.

**All provider keys are available simultaneously.** The proxy stores keys for Anthropic, OpenAI, and Gemini in parallel. If one provider is down or unconfigured, requests to other providers still work.

## Security

### Garage Identity (SPIFFE SVIDs)

Garages authenticate using their SPIFFE SVID—a short-lived Ed25519-signed JWT with:
- 15-minute TTL (auto-rotated by moto-club)
- Bound to specific pod UID
- Claims include garage ID and namespace

**Why SVIDs instead of static tokens?** A predictable identifier like `garage-{id}` would let any pod that can reach the proxy impersonate any garage. SVIDs provide cryptographic proof of identity.

The proxy validates each request by:
1. Extracting the garage ID from SVID claims
2. Verifying the signature (public key from moto-club)
3. Checking that the garage is in `Ready` state
4. Caching the result for 60 seconds

Missing or malformed tokens return `401 Unauthorized`. Expired SVIDs, non-garage callers, and non-ready garages return `403 Forbidden`.

### Secret Management

The AI proxy fetches real API keys from keybox using its own service principal SVID (`spiffe://moto.local/service/ai-proxy`).

**Secret paths in keybox:**
- `ai-proxy/anthropic` — Anthropic API key
- `ai-proxy/openai` — OpenAI API key
- `ai-proxy/gemini` — Google Gemini API key

Keys are cached in memory for 5 minutes using `SecretString` (zeroize-on-drop) types. On cache expiry, the proxy fetches fresh keys from keybox. This avoids hitting keybox on every AI request while keeping the cache window short.

**Key rotation:** When a key is rotated in keybox, the proxy picks up the new value on the next cache refresh—no restart needed.

**Missing keys:** If a provider's key isn't configured, requests to that provider return `503 Service Unavailable`. Other providers continue to work.

### Error Sanitization

All errors returned to garages use the OpenAI error format for SDK compatibility:

```json
{"error": {"message": "human-readable message", "type": "error_type"}}
```

**Provider errors are wrapped**—raw upstream error bodies are never forwarded directly. Any field that could contain API key material is scrubbed.

**API keys are never logged**, never included in error messages, and never returned in responses.

### Network Isolation

The AI proxy runs in the `moto-system` namespace. Garage NetworkPolicies already allow egress to `moto-system` (for keybox access), so garages can reach the proxy by default.

**Optional hardening (future):** Block direct internet egress to AI provider IPs from garages, forcing all AI traffic through the proxy. For now, garages can still reach AI providers directly—using the proxy is opt-in via environment variables.

## Configuration

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `MOTO_AI_PROXY_BIND_ADDR` | `0.0.0.0:8080` | Listen address |
| `MOTO_AI_PROXY_KEYBOX_URL` | `http://keybox.moto-system:8080` | Keybox endpoint |
| `MOTO_AI_PROXY_SVID_FILE` | `/var/run/secrets/svid/svid.jwt` | Path to service SVID |
| `MOTO_AI_PROXY_CLUB_URL` | `http://moto-club.moto-system:8080` | moto-club endpoint |
| `MOTO_AI_PROXY_KEY_CACHE_TTL_SECS` | `300` | API key cache duration (5 min) |
| `MOTO_AI_PROXY_GARAGE_CACHE_TTL_SECS` | `60` | Garage validation cache (1 min) |

### Local Development

When running `moto dev up`, the operator is prompted to provide API keys if they don't exist in keybox:

```
AI provider keys not found in keybox. Set them now? (or skip with --no-ai-proxy)

  ANTHROPIC_API_KEY: sk-ant-...
  OPENAI_API_KEY: (skip)
  GEMINI_API_KEY: (skip)

Stored 1 key in keybox.
```

Alternatively, set keys via environment variables before running `moto dev up`:

```bash
export MOTO_DEV_ANTHROPIC_KEY="sk-ant-..."
export MOTO_DEV_OPENAI_KEY="sk-..."
moto dev up
```

To skip the proxy entirely (and use direct API keys in garages), run:

```bash
moto dev up --no-ai-proxy
```

### Deployment

The AI proxy runs as a bike engine in `moto-system` with 2 replicas for availability:

- CPU: 100m (limit: 500m)
- Memory: 128Mi (limit: 256Mi)
- Service: `ai-proxy.moto-system:8080`
- SVID mounted at `/var/run/secrets/svid/svid.jwt`

NetworkPolicy: allow ingress from garage namespaces, allow egress to keybox and internet.

### Health Endpoints

| Endpoint | Checks |
|----------|--------|
| `/health/live` | Process alive |
| `/health/ready` | Keybox reachable, at least one provider key cached |
| `/health/startup` | SVID loaded, initial key fetch complete |

## Request Flow Details

### Timeouts

| Timeout | Default | Purpose |
|---------|---------|---------|
| Connect | 10s | TCP connection to upstream |
| First byte | 30s | Time to first response byte |
| Idle | 120s | Max time between response chunks (streaming) |
| Total | 600s | Max total request duration (10 min) |

### Streaming

The proxy fully supports Server-Sent Events (SSE) streaming:
- Sets `Transfer-Encoding: chunked` if upstream uses it
- Flushes response chunks immediately (no buffering)
- Propagates SSE events (`data:`, `event:`) without modification
- Forwards `Content-Type: text/event-stream`

### Response Headers

The proxy adds debug headers to all responses:
- `X-Moto-Request-Id: {uuid}` — correlation ID (also logged)
- `X-Moto-Provider: anthropic` — which provider handled the request

### Translation Details

For the unified endpoint, OpenAI ↔ Anthropic translation handles:

**Request:**
- System messages: OpenAI puts them in the `messages` array; Anthropic uses a top-level `system` parameter
- Field renaming: `stop` → `stop_sequences`
- Tool schemas: compatible, only wrapping format differs

**Response:**
- Token counts: `prompt_tokens`/`completion_tokens` ↔ `input_tokens`/`output_tokens`
- Stop reasons: `end_turn`→`stop`, `max_tokens`→`length`, `tool_use`→`tool_calls`
- Content extraction: Anthropic returns content blocks; OpenAI expects flat text

**Streaming:**
- Anthropic SSE events (`message_start`, `content_block_delta`, `message_stop`) are translated to OpenAI SSE chunk format (`choices[0].delta.content`, `[DONE]`)

**Gemini:** Uses OpenAI-compatible mode directly—no translation needed, only auth injection.

## Observability

Structured logs (canonical format) include:

```
level=info request_id=550e8400-... garage_id=abc123 provider=anthropic mode=passthrough method=POST path=/v1/messages status=200 duration_ms=1523 upstream_status=200
```

Log fields:
- `request_id`: Correlation ID
- `garage_id`: Requesting garage
- `provider`: AI provider name
- `mode`: `passthrough` or `unified`
- `status`: Response status returned to garage
- `upstream_status`: Response from AI provider
- `duration_ms`: Total request time

## Deferred Features

Future enhancements (not yet implemented):
- **Rate limiting per garage** (deferred to moto-throttle)
- **Usage tracking and billing**
- **Content safety / prompt filtering**
- **Provider failover** (route to backup provider if primary is down)
- **Per-garage provider restrictions** (ABAC extension)
- **WebSocket support** (OpenAI Realtime API)
- **Embeddings endpoint** (`/v1/embeddings`)
- **Vision/multimodal translation** (image content blocks differ between providers)

## See Also

- [architecture.md](architecture.md) — Component map and data flow
- [security.md](security.md) — SPIFFE SVIDs, threat model, isolation layers
- [getting-started.md](getting-started.md) — Setting up API keys during `moto dev up`
