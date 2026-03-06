# Moto Club WebSocket

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Status | Ripping |
| Last Updated | 2026-03-06 |

## Overview

WebSocket and streaming endpoints for moto-club. Provides real-time peer coordination, log streaming, and event notifications.

**This is NOT for terminal access.** Terminal access uses WireGuard tunnels to ttyd. See [moto-wgtunnel.md](moto-wgtunnel.md).

## Endpoints

| Endpoint | Auth | Direction | Status |
|----------|------|-----------|--------|
| `/internal/wg/garages/{id}/peers` | K8s ServiceAccount token | Server → garage pod | Implemented |
| `/ws/v1/garages/{name}/logs` | Same as REST API | Server → CLI client | New |
| `/ws/v1/events` | Same as REST API | Server → CLI client | New |

**Auth note:** The new endpoints use the same owner-based auth as the REST API. Real authentication (OAuth/JWT) is deferred per moto-club.md. When the auth system is implemented, these endpoints will adopt it.

## Peer Streaming (Implemented)

Internal endpoint for garage pods to receive WireGuard peer updates from moto-club.

```
WS /internal/wg/garages/{id}/peers
```

### Authentication

K8s ServiceAccount token validated via TokenReview API. The token's namespace must match `moto-garage-{garage_id}`.

### Message format

Server sends `PeerEvent` JSON messages. Uses the field name `action` (not `type`) because this was implemented first. The newer log/event endpoints use `type` as the discriminator field for consistency with standard event stream conventions.

```json
{"action": "add", "public_key": "base64...", "allowed_ip": "fd00:moto:2::1/128"}
{"action": "remove", "public_key": "base64..."}
```

- `action`: `"add"` or `"remove"`
- `public_key`: WireGuard public key (base64)
- `allowed_ip`: Present on `add`, absent on `remove`

### Connection lifecycle

1. Client connects with `Authorization: Bearer <k8s-sa-token>`
2. Server validates token, extracts garage ID from namespace
3. Server sends current peers as `add` events (initial sync)
4. Server streams new `add`/`remove` events as sessions are created/closed
5. Server handles Ping/Pong keepalive
6. On disconnect: broadcaster removes garage's channel

### Implementation

- `moto-club-ws` crate: `handle_peers_socket()` handler
- `moto-club-wg` crate: `PeerBroadcaster` with per-garage broadcast channels (capacity 64)
- Broadcaster called from `create_session` and `close_session` REST handlers
- Polling fallback: `GET /api/v1/wg/garages/{id}/peers?version=` for clients preferring HTTP

## Log Streaming (New)

WebSocket endpoint for streaming garage container logs to CLI clients.

```
WS /ws/v1/garages/{name}/logs?tail=100&follow=false
```

### Query parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `tail` | integer | `100` | Number of historical lines to send first |
| `follow` | boolean | `false` | Stream new lines after history |
| `since` | string | | Relative duration (e.g., `5m`, `1h`) |

### Garage state requirements

| State | Behavior |
|-------|----------|
| Pending | Error: `{"type": "error", "message": "garage not ready"}` |
| Initializing | Allow — init container logs may help diagnose issues |
| Ready | Allow |
| Failed | Allow — logs help diagnose what went wrong |
| Terminated | Error: `{"type": "error", "message": "garage terminated"}` |

### Message format

```json
{"type": "log", "timestamp": "2026-01-21T10:15:32Z", "line": "Starting dev environment..."}
{"type": "error", "message": "Pod not found"}
{"type": "eof", "reason": "pod_terminated"}
{"type": "dropped", "count": 42, "message": "client too slow, 42 lines dropped"}
```

### How it works

1. Client connects, server validates auth and resolves garage → namespace/pod
2. Server opens K8s pod log stream (same mechanism as `moto garage logs -f`)
3. Historical lines sent first (based on `tail` parameter)
4. If `follow=true`, new lines streamed as they arrive
5. If `follow=false`, send all historical lines then `eof` with `reason: "complete"`
6. If pod terminates or restarts, send `eof` message with reason
7. Backpressure: buffer up to 256 messages. If client falls behind, drop oldest undelivered messages and send a `dropped` message with the count.

### Connection limits

Maximum 5 concurrent log streaming WebSocket connections per garage.

### Why WebSocket instead of K8s API directly

The CLI currently uses direct K8s API access for `moto garage logs`. WebSocket through moto-club:
- Works without kubeconfig (only needs moto-club URL + auth token)
- Consistent auth model
- Required for remote/hosted deployments where clients don't have K8s access

The CLI should try WebSocket first, fall back to direct K8s API for local dev.

## Event Streaming (New)

WebSocket endpoint for real-time notifications about garages and system events.

```
WS /ws/v1/events?garages=bold-mongoose,quiet-falcon
```

### Query parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `garages` | string | all | Comma-separated garage names to watch (empty = all owned) |

### Event types

**TTL warnings** — sent at 15 minutes and 5 minutes before expiry:

```json
{"type": "ttl_warning", "garage": "bold-mongoose", "minutes_remaining": 15, "expires_at": "2026-01-21T12:00:00Z"}
{"type": "ttl_warning", "garage": "bold-mongoose", "minutes_remaining": 5, "expires_at": "2026-01-21T12:00:00Z"}
```

**Status changes** — emitted on any state transition:

```json
{"type": "status_change", "garage": "bold-mongoose", "from": "Pending", "to": "Initializing"}
{"type": "status_change", "garage": "bold-mongoose", "from": "Initializing", "to": "Ready"}
{"type": "status_change", "garage": "bold-mongoose", "from": "Initializing", "to": "Failed", "reason": "clone_failed"}
{"type": "status_change", "garage": "bold-mongoose", "from": "Ready", "to": "Terminated", "reason": "ttl_expired"}
{"type": "status_change", "garage": "bold-mongoose", "from": "Ready", "to": "Terminated", "reason": "user_closed"}
```

The `reason` field is present only on transitions to Terminated or Failed. Values match `TerminationReason` enum: `user_closed`, `ttl_expired`, `pod_lost`, `namespace_missing`, `error`.

**Errors:**

```json
{"type": "error", "garage": "bold-mongoose", "message": "Pod crash loop detected"}
```

### Event sources

| Event | Source | Trigger |
|-------|--------|---------|
| `ttl_warning` | Reconciler | Checks `expires_at - now() < threshold` each cycle |
| `status_change` | Garage service + Reconciler | On any status transition |
| `error` | Reconciler | Pod failures, crash loops |

### Reconnect behavior

Events are fire-and-forget — no persistence, no replay on reconnect. On reconnect, clients should:
1. Fetch current garage state via `GET /api/v1/garages` (REST)
2. Check TTL remaining and compute if any warnings were missed
3. Resume receiving new events via WebSocket

### Connection limits

Maximum 3 concurrent event streaming WebSocket connections per user.

### Implementation approach

- Use a broadcast channel per user (similar to PeerBroadcaster pattern)
- Reconciler and garage service publish events; WebSocket handler subscribes
- Events filtered to garages owned by the authenticated user

## Deferred Items

- **Binary WebSocket frames** for high-throughput log streaming (future optimization)
- **Server-Sent Events (SSE)** alternative for event streaming (simpler for HTTP-only clients)
- **WebSocket compression** via permessage-deflate (future, when log volume warrants it)
- **Garage daemon WS client** — the daemon has `peer_stream_url()` and `handle_peer_action()` scaffolded but `run()` is a placeholder. Connecting to the peer streaming endpoint is tracked in moto-wgtunnel.md, not here.

## References

- [moto-club.md](moto-club.md) — Server architecture, API auth
- [moto-wgtunnel.md](moto-wgtunnel.md) — WireGuard tunnel system, garage daemon
- [garage-lifecycle.md](garage-lifecycle.md) — Garage state machine
- [moto-cron.md](moto-cron.md) — TTL enforcement (triggers ttl_warning and status_change events)

## Changelog

### v0.3 (2026-03-06)
- Fix: Auth for new endpoints uses same owner-based auth as REST API (real auth deferred)
- Fix: Log streaming `follow` defaults to `false` (matches CLI behavior)
- Add: Document `action` vs `type` field name difference between peer and new endpoints
- Add: Garage state requirements for log streaming (Pending rejected, Failed allowed, etc.)
- Add: `dropped` message type for log backpressure notification (buffer size 256)
- Add: Connection limits (5 per garage for logs, 3 per user for events)
- Add: All state machine transitions in status_change examples, `reason` field definition
- Add: Reconnect behavior for event streaming (REST for current state, then WS for new events)

### v0.2 (2026-03-05)
- Full spec. Document implemented peer streaming (PeerBroadcaster, handle_peers_socket, K8s SA auth). Define new log streaming endpoint (/ws/v1/garages/{name}/logs) wrapping K8s pod log API. Define new event streaming endpoint (/ws/v1/events) for TTL warnings and status changes. CLI should prefer WebSocket over direct K8s API for logs.

### v0.1 (2026-01-22)
- Bare frame placeholder
