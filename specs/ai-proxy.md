# AI Proxy

| | |
|--------|----------------------------------------------|
| Version | 0.5 |
| Status | Ready to Rip |
| Last Updated | 2026-03-06 |

## Overview

HTTP reverse proxy between garages and AI providers (Anthropic, OpenAI, Gemini). Injects API credentials from keybox so garages never see real API keys. Runs as a shared service in `moto-system` namespace.

**Key properties:**
- **Two access modes**: provider-native passthrough (primary for Claude Code) and OpenAI-compatible unified endpoint (for multi-provider tools)
- All provider API keys stored in keybox simultaneously
- Proxy **auto-routes by model name** on the unified endpoint — `claude-*` → Anthropic, `gpt-*` → OpenAI, `gemini-*` → Gemini
- Proxy fetches real API keys from keybox via SVID authentication
- Per-garage request identity for audit trail and rate limiting (future)

**What this is NOT:**
- Not a prompt filter or content safety layer (future)
- Not a caching layer

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Garage Pod (moto-garage-{id})                                   │
│                                                                   │
│  Claude Code (primary workload)                                  │
│  └── ANTHROPIC_BASE_URL=http://ai-proxy.moto-system:8080/       │
│      passthrough/anthropic                                       │
│  └── ANTHROPIC_API_KEY=garage-{garage_id}                        │
│                                                                   │
│  Other AI tools (litellm, langchain, openai SDK, etc.)           │
│  └── OPENAI_BASE_URL=http://ai-proxy.moto-system:8080/v1        │
│  └── OPENAI_API_KEY=garage-{garage_id}                           │
└───────────────────────────┬───────────────────────────────────────┘
                            │ HTTP (cluster-internal)
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│  ai-proxy (moto-system namespace)                                │
│                                                                   │
│  Passthrough routes (provider-native, no translation):           │
│  └── /passthrough/anthropic/ → api.anthropic.com                 │
│  └── /passthrough/openai/   → api.openai.com                    │
│  └── /passthrough/gemini/   → googleapis.com                    │
│                                                                   │
│  Unified endpoint (OpenAI format, auto-routes by model name):    │
│  └── /v1/chat/completions → routes by model prefix, translates  │
│                                                                   │
│  All routes: validate garage identity, inject real API key       │
└──────────┬────────────────┬────────────────┬─────────────────────┘
           │                │                │
           ▼                ▼                ▼
   api.anthropic.com  api.openai.com  googleapis.com
```

## Passthrough Routes (Primary)

Passthrough routes forward requests directly to a provider's API with credential injection. No request/response translation — the garage uses the provider's native SDK and format.

**This is the primary path for Claude Code**, which uses the Anthropic SDK natively.

### Garage configuration for Claude Code

```bash
ANTHROPIC_BASE_URL=http://ai-proxy.moto-system:8080/passthrough/anthropic
ANTHROPIC_API_KEY=garage-{garage_id}
```

Claude Code sends Anthropic-native requests. The proxy strips the garage token, injects the real `x-api-key`, and forwards to `api.anthropic.com`. Responses stream back unchanged.

### Example: Claude Code via passthrough

```
Garage sends (Anthropic native format):
  POST http://ai-proxy.moto-system:8080/passthrough/anthropic/v1/messages
  x-api-key: garage-{garage_id}
  anthropic-version: 2023-06-01
  Content-Type: application/json
  {
    "model": "claude-sonnet-4-20250514",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true
  }

Proxy rewrites to:
  POST https://api.anthropic.com/v1/messages
  x-api-key: sk-ant-real-key-from-keybox
  anthropic-version: 2023-06-01
  Content-Type: application/json
  {same body, unchanged}
```

### Available passthrough routes

| Route | Upstream | Auth rewrite |
|-------|----------|-------------|
| `/passthrough/anthropic/` | `https://api.anthropic.com/` | Replace `x-api-key` or `Authorization` with real key via `x-api-key: {key}` |
| `/passthrough/openai/` | `https://api.openai.com/` | Replace `Authorization` with `Bearer {key}` |
| `/passthrough/gemini/` | `https://generativelanguage.googleapis.com/` | Replace auth with `x-goog-api-key: {key}` |

### Path allowlist

Passthrough routes only forward to provider API paths, not admin/billing endpoints:

| Provider | Allowed path prefixes |
|----------|----------------------|
| Anthropic | `/v1/messages`, `/v1/complete` |
| OpenAI | `/v1/chat/`, `/v1/models`, `/v1/embeddings` |
| Gemini | `/v1beta/`, `/v1/` |

Requests to paths outside the allowlist return `403` with `{"error": {"message": "path not allowed", "type": "forbidden"}}`.

## Unified Endpoint (Multi-Provider)

For tools that use the OpenAI-compatible API format (litellm, langchain ChatOpenAI, openai SDK), the proxy provides a unified endpoint that auto-routes by model name.

### Garage configuration for OpenAI-compatible tools

```bash
OPENAI_BASE_URL=http://ai-proxy.moto-system:8080/v1
OPENAI_API_KEY=garage-{garage_id}
```

### Example: OpenAI SDK calling Claude via unified endpoint

```
Garage sends (OpenAI format):
  POST http://ai-proxy.moto-system:8080/v1/chat/completions
  Authorization: Bearer garage-{garage_id}
  Content-Type: application/json
  {
    "model": "claude-sonnet-4-20250514",
    "messages": [{"role": "user", "content": "Hello"}],
    "stream": true
  }

Proxy translates and sends to Anthropic:
  POST https://api.anthropic.com/v1/messages
  x-api-key: sk-ant-real-key-from-keybox
  anthropic-version: 2023-06-01
  {translated body}

Proxy translates response back to OpenAI format.
```

### Example: OpenAI SDK calling GPT (no translation)

```
Garage sends:
  POST http://ai-proxy.moto-system:8080/v1/chat/completions
  Authorization: Bearer garage-{garage_id}
  Content-Type: application/json
  {
    "model": "gpt-4o",
    "messages": [{"role": "user", "content": "Hello"}]
  }

Proxy forwards directly:
  POST https://api.openai.com/v1/chat/completions
  Authorization: Bearer sk-real-key-from-keybox
  {same body, unchanged}
```

## Model-Based Routing

The proxy inspects the `model` field in the request body and routes to the correct provider automatically. All provider keys are available simultaneously.

### Model prefix → provider mapping

| Model Prefix | Provider | Upstream | Translation |
|-------------|----------|----------|-------------|
| `claude-*` | Anthropic | `https://api.anthropic.com/` | OpenAI ↔ Anthropic Messages API |
| `gpt-*`, `o1-*`, `o3-*`, `chatgpt-*` | OpenAI | `https://api.openai.com/` | None (native format) |
| `gemini-*` | Gemini | `https://generativelanguage.googleapis.com/` | None (Gemini OpenAI-compat mode) |

**Unknown model prefix:** Return `400` with `{"error": {"message": "unknown model prefix, cannot determine provider", "type": "invalid_request_error"}}` (OpenAI error format).

### Provider auth headers

| Provider | Auth Header |
|----------|-------------|
| Anthropic | `x-api-key: {key}` |
| OpenAI | `Authorization: Bearer {key}` |
| Gemini | `x-goog-api-key: {key}` |

### All routes

| Path | Mode | Behavior |
|------|------|----------|
| `/passthrough/anthropic/**` | Passthrough | Forward to Anthropic API (primary for Claude Code) |
| `/passthrough/openai/**` | Passthrough | Forward to OpenAI API |
| `/passthrough/gemini/**` | Passthrough | Forward to Gemini API |
| `/v1/chat/completions` | Unified | Auto-route by model name, translate if needed |
| `/v1/models` | Unified | Return merged model list from all configured providers |

### Missing provider key

If a garage requests a model (or passthrough route) whose provider has no key in keybox, return `503` with `{"error": {"message": "provider not configured: anthropic", "type": "server_error"}}`. The proxy still works for other providers — a missing OpenAI key doesn't block Anthropic requests.

### Fine-tuned model names

OpenAI fine-tuned models use the format `ft:gpt-4o:org:model:id`. The proxy strips the `ft:` prefix before matching, so `ft:gpt-4o:...` routes to OpenAI. If a model name contains a colon, the prefix before the first colon is checked for `ft:` and stripped if present.

## Translation Layer

### OpenAI → Anthropic translation

The proxy translates between OpenAI `chat/completions` format and Anthropic `messages` format.

**Request translation:**

| OpenAI field | Anthropic field | Notes |
|--------------|-----------------|-------|
| `model` | `model` | Passed through unchanged |
| `messages` | `messages` | Role mapping: `system` → extracted to top-level `system` param |
| `messages[].content` | `messages[].content` | String or content blocks (both supported) |
| `max_tokens` | `max_tokens` | Direct mapping |
| `temperature` | `temperature` | Direct mapping |
| `top_p` | `top_p` | Direct mapping |
| `stream` | `stream` | Direct mapping |
| `stop` | `stop_sequences` | Rename only |
| `tools` | `tools` | Tool schema is compatible |
| `tool_choice` | `tool_choice` | Format differs slightly — translate |

**System message handling:** OpenAI puts system messages in the `messages` array. Anthropic uses a top-level `system` parameter. The proxy extracts `{"role": "system", ...}` messages and moves them to the `system` field.

**Response translation (non-streaming):**

| Anthropic field | OpenAI field | Notes |
|-----------------|--------------|-------|
| `id` | `id` | Passed through |
| `content[0].text` | `choices[0].message.content` | Extract text from content blocks |
| `model` | `model` | Passed through |
| `stop_reason` | `choices[0].finish_reason` | Map: `end_turn`→`stop`, `max_tokens`→`length`, `tool_use`→`tool_calls` |
| `usage.input_tokens` | `usage.prompt_tokens` | Rename |
| `usage.output_tokens` | `usage.completion_tokens` | Rename |

**Response translation (streaming SSE):**

Anthropic SSE events are translated to OpenAI SSE chunk format:

| Anthropic event | OpenAI chunk |
|-----------------|-------------|
| `message_start` | Initial chunk with `role: "assistant"` |
| `content_block_delta` (text) | `choices[0].delta.content` |
| `content_block_delta` (tool_use) | `choices[0].delta.tool_calls` |
| `message_delta` (stop_reason) | `choices[0].finish_reason` |
| `message_stop` | `[DONE]` |

**Tool use translation:** OpenAI and Anthropic tool schemas are structurally similar. The proxy translates the wrapping format but does not modify tool definitions or arguments.

### OpenAI → Gemini translation

Gemini supports an OpenAI-compatible endpoint (`/v1beta/openai/chat/completions`). The proxy routes to this endpoint directly — no request/response translation needed. Only auth header injection is required.

### Unsupported features

Features that exist in one provider but not others are handled as follows:

- **Provider-specific parameters** in the request body (e.g., Anthropic's `metadata`, OpenAI's `logprobs`) → stripped by translation, logged at debug level
- **If a feature is critical** → use the passthrough route instead of the unified endpoint

## Garage Identity and Auth

Garages authenticate to ai-proxy using their SVID JWT (already provisioned by moto-club and mounted at `/var/run/secrets/svid/`).

**Request auth:**
- Garage sends its API key (which is the SVID JWT or a per-garage token) in the provider-native auth header:
  - Passthrough Anthropic: `x-api-key: {garage-token}`
  - Passthrough OpenAI / Unified: `Authorization: Bearer {garage-token}`
- Proxy validates the token by checking it against moto-club: `GET /api/v1/garages/{id}` (garage ID extracted from SVID claims)
- Cache validation result for 60 seconds (garage state doesn't change frequently)

**Why SVID?** Garages run arbitrary user code. A predictable identifier like `garage-{id}` would let any pod that can reach ai-proxy impersonate any garage. SVIDs are cryptographically signed JWTs issued per-garage, providing real identity verification.

**Garage environment variables** use the SVID token as the API key value:

```bash
# Claude Code (passthrough)
ANTHROPIC_API_KEY={svid-jwt}

# OpenAI-compatible tools (unified)
OPENAI_API_KEY={svid-jwt}
```

The garage entrypoint reads the SVID from `/var/run/secrets/svid/svid.jwt` and sets these env vars.

**Rejected requests:**
- Missing or invalid auth → `401`
- SVID expired or garage not in `Ready` state → `403`
- Provider not configured → `503`

## Secret Management

ai-proxy fetches API keys from keybox using its own SVID (service principal).

**SPIFFE identity:** `spiffe://moto.local/service/ai-proxy`

**Secret naming convention:** `ai-proxy/{provider}` — follows the ABAC convention where the service name is the secret path prefix.

| Secret Path | Provider | Scope |
|-------------|----------|-------|
| `ai-proxy/anthropic` | Anthropic | Global |
| `ai-proxy/openai` | OpenAI | Global |
| `ai-proxy/gemini` | Google Gemini | Global |

**Key caching:** Proxy caches API keys in memory for 5 minutes (configurable). On cache miss or expiry, fetches from keybox. This avoids hitting keybox on every AI request.

**Key rotation:** When keybox returns a new key value, the cache is updated transparently. No proxy restart needed.

**Missing key:** If keybox returns 404 for a provider's key, requests to that provider return `503` with `{"error": {"message": "provider not configured: anthropic", "type": "server_error"}}`.

## Request Handling

### Header handling

**Forwarded from garage:**
- `Content-Type` header
- `Accept` header

**Stripped/replaced:**
- `Authorization` header (replaced with provider-specific auth)
- `Host` header (set to upstream host)
- Provider-specific headers added by translation layer (e.g., `anthropic-version`)

**Added to response (not sent to provider):**
- `X-Moto-Request-Id: {uuid}` — correlation ID for debugging (also logged)
- `X-Moto-Provider: anthropic` — which provider handled the request

### Streaming

AI providers return streaming responses (SSE). The proxy MUST support streaming pass-through:

- Set `Transfer-Encoding: chunked` if upstream uses it
- Flush response chunks immediately (no buffering)
- Propagate SSE events (`data:`, `event:`) without modification
- Forward upstream `Content-Type` (usually `text/event-stream` for streaming)

### Timeouts

| Timeout | Default | Purpose |
|---------|---------|---------|
| Connect | 10s | TCP connection to upstream |
| First byte | 30s | Time to first response byte |
| Idle | 120s | Max time between response chunks (streaming) |
| Total | 600s | Max total request duration (10 min) |

### Request size limits

| Limit | Default |
|-------|---------|
| Max request body | 10 MB |
| Max response body | None (streaming) |

### Error sanitization

All errors returned to garages use the OpenAI error format for SDK compatibility:

```json
{"error": {"message": "human-readable message", "type": "error_type"}}
```

Error types: `invalid_request_error`, `authentication_error`, `forbidden`, `server_error`.

**Provider errors** are wrapped — raw upstream error bodies are never forwarded directly. The proxy extracts the error message and wraps it in the standard format. Any field that could contain API key material is scrubbed.

**API key caching** uses `SecretString` (zeroize-on-drop) types. Keys are never logged, never included in error messages, and never returned in responses.

## Local Development

### With `moto dev up`

`moto dev up` starts ai-proxy as part of the local dev stack (after keybox, before opening a garage):

```
[6.5/9] Starting ai-proxy...         healthy (localhost:18090)
```

In local dev, ai-proxy runs as a `cargo run` process alongside moto-club and keybox. It connects to the local keybox instance for API keys.

### Seeding API keys locally

During `moto dev up`, if ai-proxy keys don't exist in keybox, the operator is prompted:

```
AI provider keys not found in keybox. Set them now? (or skip with --no-ai-proxy)

  ANTHROPIC_API_KEY: sk-ant-...
  OPENAI_API_KEY: (skip)
  GEMINI_API_KEY: (skip)

Stored 1 key in keybox.
```

Alternatively, set keys via environment before `moto dev up`:

```bash
export MOTO_DEV_ANTHROPIC_KEY="sk-ant-..."
export MOTO_DEV_OPENAI_KEY="sk-..."
moto dev up
```

### Without ai-proxy

If no AI keys are configured, `moto dev up --no-ai-proxy` skips the proxy entirely. Garages use direct API keys (the v1 model — user provides their own `ANTHROPIC_API_KEY` in their env).

## Deployment

Runs as a bike engine in `moto-system` namespace, following the same pattern as keybox.

### bike.toml

```toml
[engine]
name = "ai-proxy"
image = "moto-ai-proxy"

[engine.resources]
cpu = "100m"
memory = "128Mi"
cpu_limit = "500m"
memory_limit = "256Mi"

[engine.replicas]
count = 2

[[engine.ports]]
name = "http"
container = 8080
service = 8080
```

### K8s Resources

- **Deployment** in `moto-system` namespace (2 replicas for availability)
- **Service** `ai-proxy` on port 8080
- **ServiceAccount** with SVID secret mounted
- **NetworkPolicy**: allow ingress from garage namespaces, allow egress to keybox and internet

### NetworkPolicy for garages

Garage NetworkPolicy already allows egress to `moto-system` namespace (for keybox access). ai-proxy runs in `moto-system`, so no NetworkPolicy changes needed for garages.

If we want to restrict garages to ONLY use ai-proxy for AI access (block direct internet egress to AI provider IPs), that's a future hardening step. For v1, garages can still reach the internet directly — ai-proxy is opt-in via environment variables.

## Configuration

| Variable | Default | Purpose |
|----------|---------|---------|
| `MOTO_AI_PROXY_BIND_ADDR` | `0.0.0.0:8080` | Listen address |
| `MOTO_AI_PROXY_KEYBOX_URL` | `http://keybox.moto-system:8080` | Keybox endpoint |
| `MOTO_AI_PROXY_SVID_FILE` | `/var/run/secrets/svid/svid.jwt` | Path to ai-proxy SVID |
| `MOTO_AI_PROXY_CLUB_URL` | `http://moto-club.moto-system:8080` | moto-club endpoint (for garage validation) |
| `MOTO_AI_PROXY_KEY_CACHE_TTL_SECS` | `300` | API key cache duration |
| `MOTO_AI_PROXY_GARAGE_CACHE_TTL_SECS` | `60` | Garage validation cache duration |
| `MOTO_AI_PROXY_MODEL_MAP` | (built-in defaults) | Custom model prefix → provider mappings (see below) |

### Custom model mappings

For models that don't follow standard naming conventions:

```bash
MOTO_AI_PROXY_MODEL_MAP='[
  {"prefix": "mistral-", "provider": "mistral", "upstream": "https://api.mistral.ai/", "auth_header": "Authorization", "auth_prefix": "Bearer "}
]'
```

Each custom provider also needs a corresponding `ai-proxy/{provider}` secret in keybox.

## Health Endpoints

Standard bike health endpoints:

| Endpoint | Checks |
|----------|--------|
| `/health/live` | Process alive |
| `/health/ready` | Keybox reachable, at least one provider key cached |
| `/health/startup` | SVID loaded, initial key fetch complete |

## Observability

**Logging (structured, canonical):**

```
level=info request_id=550e8400-e29b-41d4-a716-446655440000 garage_id=abc123 provider=anthropic mode=passthrough method=POST path=/v1/messages status=200 duration_ms=1523 upstream_status=200 tokens_in=150 tokens_out=500
```

Log fields:
- `request_id`: Correlation ID (returned in `X-Moto-Request-Id` response header)
- `garage_id`: Requesting garage
- `provider`: AI provider name
- `mode`: `passthrough` or `unified`
- `method`, `path`: Original request
- `status`: Response status returned to garage
- `upstream_status`: Response from AI provider
- `duration_ms`: Total request time
- `tokens_in`, `tokens_out`: Token counts from provider response headers (if available, provider-specific)

**Metrics (future):** Request count, latency histogram, error rate by provider. Deferred to moto-throttle.md.

## Crate Structure

```
crates/
└── moto-ai-proxy/
    └── src/
        ├── main.rs           # Binary entrypoint, config loading
        ├── lib.rs             # Library root
        ├── config.rs          # Configuration parsing
        ├── proxy.rs           # Core proxy logic (forward, stream)
        ├── provider.rs        # Provider registry, backend selection
        ├── translate/
        │   ├── mod.rs         # Translation trait
        │   ├── anthropic.rs   # OpenAI ↔ Anthropic translation
        │   └── passthrough.rs # No-op translation (OpenAI, Gemini compat)
        ├── auth.rs            # Garage identity validation
        ├── keys.rs            # Keybox client, key caching
        └── health.rs          # Health endpoints
```

## Deferred Items

- **Rate limiting per garage** — defer to moto-throttle.md
- **Usage tracking and billing** — future, requires token counting integration
- **Content safety / prompt filtering** — future
- **Provider failover** (e.g., Anthropic down → route to OpenAI for same model class) — future
- **Per-garage provider restrictions** (e.g., garage X can only use Anthropic) — future, ABAC extension
- **Block direct AI API egress** — future hardening, restrict garage NetworkPolicy to force ai-proxy usage
- **WebSocket provider support** (e.g., OpenAI Realtime API) — future
- **Embeddings endpoint** (`/v1/embeddings`) — future, when embedding use cases arise
- **Vision/multimodal translation** — image content blocks differ between providers, translate when needed

## References

- [keybox.md](keybox.md) — Secret storage, SVID authentication, ABAC policies
- [garage-isolation.md](garage-isolation.md) — Network policies, egress rules
- [moto-bike.md](moto-bike.md) — Engine contract, health endpoints
- [service-deploy.md](service-deploy.md) — K8s deployment patterns
- [moto-throttle.md](moto-throttle.md) — Rate limiting (future)

## Changelog

### v0.5 (2026-03-06)
- Reframe: passthrough routes are primary path (Claude Code uses Anthropic SDK natively)
- Unified endpoint is for OpenAI-compatible tools (litellm, langchain, etc.)
- Security: use garage SVID for auth (not predictable `garage-{id}` token)
- Security: passthrough path allowlist (block admin/billing endpoints)
- Security: error sanitization, `SecretString` for cached keys
- Add: `X-Moto-Request-Id` and `X-Moto-Provider` response headers for debugging
- Add: Local Development section (moto dev up integration, key seeding)
- Fix: consistent OpenAI error format across all error responses
- Add: fine-tuned model name handling (`ft:gpt-4o:...` strips `ft:` prefix)

### v0.4 (2026-03-06)
- Model-based auto-routing: proxy inspects `model` field and routes to correct provider
- All provider keys stored simultaneously — no single-backend limitation
- Remove `MOTO_AI_PROXY_BACKEND` env var (routing is automatic)
- Add `MOTO_AI_PROXY_MODEL_MAP` for custom model prefix → provider mappings
- Missing provider key returns 503 for that provider only, others still work

### v0.3 (2026-03-06)
- Redesign: provider-agnostic unified endpoint using OpenAI-compatible format
- Garages send to one URL (`/v1/chat/completions`), proxy translates to backend provider
- Add translation layer spec: OpenAI ↔ Anthropic (request/response/streaming), Gemini via compat mode
- Add passthrough routes (`/passthrough/{provider}/`) for provider-native API access

### v0.2 (2026-03-06)
- Full spec. Path-based provider routing, keybox secret injection via SVID, garage identity validation via moto-club, streaming pass-through, key caching, bike engine deployment in moto-system namespace.

### v0.1 (2026-01-19)
- Bare frame placeholder
