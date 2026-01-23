# Moto Club WebSocket

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Bare Frame |
| Last Updated | 2026-01-22 |

## Overview

WebSocket streaming endpoints for moto-club. Provides real-time log streaming, event notifications (TTL warnings, status changes), and internal peer coordination for WireGuard.

**This is NOT for terminal access.** Terminal/SSH access uses WireGuard tunnels directly. See [moto-wgtunnel.md](moto-wgtunnel.md).

## Specification

### Endpoints

```
/ws/v1/garages/{name}/logs          Stream garage logs
/ws/v1/events                       Real-time events (TTL warnings, status changes)
/internal/wg/garages/{id}/peers     Peer streaming (garage pod → moto-club)
```

### Log Streaming

**TODO:** Define message format, filtering options, backpressure handling.

```
/ws/v1/garages/{name}/logs?tail=100&follow=true
```

### Event Streaming

**TODO:** Define event types, subscription model, message format.

Events to support:
- TTL warnings (15 min, 5 min before expiry)
- Garage status changes (Pending → Running → Ready → Terminated)
- Error notifications

### Peer Streaming (Internal)

**TODO:** Define protocol for garage pods to receive peer updates from moto-club.

Used by garage WireGuard daemon to dynamically add/remove client peers.

```
{ "action": "add", "public_key": "...", "allowed_ip": "fd00:moto:2::1/128" }
{ "action": "remove", "public_key": "..." }
```

## References

- [moto-club.md](moto-club.md) - Main moto-club specification
- [moto-wgtunnel.md](moto-wgtunnel.md) - WireGuard tunnel system
- [garage-lifecycle.md](garage-lifecycle.md) - Garage state machine
