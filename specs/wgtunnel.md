# WireGuard Tunnel System

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Bare Frame |
| Last Updated | 2026-01-21 |

## Overview

Secure connectivity layer for terminal/SSH access to garages. moto-club coordinates only, never sees traffic.

**This is for terminal access only.** Streaming logs, TTL warnings, and events use WebSocket/SSE (see moto-club.md).

## Specification

_To be written_

## Notes

**Why WireGuard over WebSocket:**
- Real SSH sessions, not proxied terminals
- P2P when possible, relay only for NAT traversal
- Any TCP port forwarding (not just terminal)
- Server never sees traffic (coordination only)

**Key decisions needed:**
- IP range (loom uses `fd7a:115c:a1e0::/48`)
- DERP relay hosting strategy
- Key storage location (`~/.config/moto/wg-key`?)

**References:**
- Loom's wgtunnel-system.md
- boringtun (userspace WireGuard)
- DERP protocol (Tailscale's relay)
