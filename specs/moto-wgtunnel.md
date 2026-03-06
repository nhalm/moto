# WireGuard Tunnel System

| | |
|--------|----------------------------------------------|
| Version | 0.10 |
| Status | Ripping |
| Last Updated | 2026-03-06 |

## Overview

Secure connectivity layer for WebSocket terminal (ttyd) access to garages. WireGuard provides encrypted P2P tunnels with DERP relay fallback for NAT traversal. moto-club coordinates peer discovery and IP allocation but never sees traffic.

**This is for terminal access only.** Streaming logs, TTL warnings, and events use WebSocket/SSE (see moto-club.md).

**Key properties:**
- WebSocket terminal (ttyd) over encrypted WireGuard tunnel
- P2P when possible, DERP relay when NAT blocks direct connection
- Userspace implementation - no sudo/root required on client
- Server never sees traffic (coordination only)
- Works across firewalls and NATs

**Supported platforms:** Linux, macOS (no Windows support)

## Design Principles

| Principle | Description |
|-----------|-------------|
| **P2P when possible** | Direct WireGuard UDP connections preferred |
| **Relay when needed** | Self-hosted DERP servers as encrypted fallback |
| **Zero server relay** | moto-club coordinates only, never sees traffic |
| **Ephemeral garage keys** | New WireGuard keypair per garage pod |
| **Persistent device keys** | User devices keep stable identity |
| **Tunnel = auth** | WireGuard tunnel establishment is the authentication boundary |

## Specification

### Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              User Device                                     │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  moto-cli                                                            │    │
│  │  ├── WireGuard Engine (boringtun, userspace)                         │    │
│  │  ├── DERP Client                                                     │    │
│  │  ├── MagicConn (direct UDP + DERP multiplexer)                       │    │
│  │  ├── Virtual TUN (in-process, no kernel device)                      │    │
│  │  └── Device WG Keypair (~/.config/moto/wg-private.key)               │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
                                  │
                    ┌─────────────┴─────────────┐
                    │                           │
                    ▼                           ▼
    ┌───────────────────────────┐   ┌───────────────────────────────┐
    │   Direct UDP (preferred)   │   │   DERP Relay (fallback)       │
    │   - 3 second timeout       │   │   - Self-hosted               │
    │   - No upgrade after DERP  │   │   - Try other regions on fail │
    └───────────────────────────┘   └───────────────────────────────┘
                    │                           │
                    └─────────────┬─────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              Garage Pod                                      │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  moto-garage-wgtunnel daemon                                         │    │
│  │  ├── WireGuard Engine (boringtun, userspace)                         │    │
│  │  ├── DERP Client                                                     │    │
│  │  ├── MagicConn                                                       │    │
│  │  ├── Ephemeral WG Keypair (in-memory)                                │    │
│  │  └── Health endpoint (/health)                                       │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │  Terminal daemon (ttyd + tmux, no auth - tunnel is auth boundary)    │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
                                  ▲
                                  │ Control Plane Only
                                  │ (peer streaming, no traffic relay)
                                  ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              moto-club                                       │
│  ┌─────────────────────────────────────────────────────────────────────┐    │
│  │  Coordination APIs                                                   │    │
│  │  ├── Device Registration (POST /api/v1/wg/devices)                   │    │
│  │  ├── Garage WG Registration (POST /api/v1/wg/garages)                │    │
│  │  ├── Session Creation (POST /api/v1/wg/sessions)                     │    │
│  │  ├── Peer Streaming WebSocket (/internal/wg/garages/{id}/peers)      │    │
│  │  ├── IP Allocator (fd00:moto::/48)                                   │    │
│  │  └── DERP Map Provider                                               │    │
│  └─────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Crate Structure

```
crates/
├── moto-wgtunnel-types/        # Shared types (keys, IPs, DERP maps)
│   └── src/
│       ├── lib.rs
│       ├── keys.rs             # WireGuard key types
│       ├── ip.rs               # IP allocation types
│       ├── peer.rs             # Peer information
│       └── derp.rs             # DERP map types
│
├── moto-wgtunnel-derp/         # DERP protocol implementation
│   └── src/
│       ├── lib.rs
│       ├── client.rs           # DERP client
│       ├── protocol.rs         # Frame encoding/decoding
│       └── map.rs              # DERP server map
│
├── moto-wgtunnel-conn/         # Connection multiplexer
│   └── src/
│       ├── lib.rs
│       ├── magic.rs            # MagicConn: UDP + DERP multiplexer
│       ├── stun.rs             # STUN for NAT discovery
│       ├── endpoint.rs         # Endpoint selection logic
│       └── path.rs             # Path status (Direct/Derp)
│
├── moto-wgtunnel-engine/       # WireGuard engine (boringtun)
│   └── src/
│       ├── lib.rs
│       ├── tunnel.rs           # Tunnel management
│       ├── config.rs           # WireGuard configuration
│       └── platform/           # Platform-specific TUN abstractions
│           ├── mod.rs
│           ├── linux.rs
│           └── macos.rs
│
├── moto-cli-wgtunnel/          # CLI wgtunnel integration
│   └── src/
│       ├── lib.rs
│       ├── enter.rs            # garage enter command
│       ├── tunnel.rs           # tunnel management
│       └── status.rs           # connection status
│
├── moto-club-wg/               # Server-side coordination (in moto-club)
│   └── src/
│       ├── lib.rs
│       ├── peers.rs            # Peer registration
│       ├── ipam.rs             # IP address allocation
│       ├── sessions.rs         # Tunnel session management
│       └── derp.rs             # DERP map management
│
└── moto-garage-wgtunnel/       # Garage-side daemon
    └── src/
        ├── lib.rs
        ├── daemon.rs           # Main daemon loop
        ├── register.rs         # Register with moto-club
        └── health.rs           # Health endpoint
```

**Dependency graph:**

```
                    moto-cli                    moto-garage
                        │                           │
                        ▼                           ▼
              moto-cli-wgtunnel          moto-garage-wgtunnel
                        │                           │
                        └─────────┬─────────────────┘
                                  │
                                  ▼
                        moto-wgtunnel-engine
                                  │
                        ┌─────────┴─────────┐
                        │                   │
                        ▼                   ▼
              moto-wgtunnel-conn    moto-wgtunnel-types
                        │
                        ▼
              moto-wgtunnel-derp
```

### IP Allocation

**Overlay network:** `fd00:moto::/48` (IPv6 ULA, private to moto)

| Subnet | Range | Purpose |
|--------|-------|---------|
| Garages | `fd00:moto:1::/64` | Garage pods |
| Clients | `fd00:moto:2::/64` | User devices |

**Allocation strategy:**

- **Garages:** IP derived from garage ID (deterministic hash)
- **Clients:** Allocated per WireGuard public key, persisted. Same key re-registering gets same IP.

**Why IPv6 ULA:**
- No collision with public IPs
- Large address space (no exhaustion concerns)
- Standard prefix recognized by networking tools

### Key Management

**Client WireGuard keys (persistent):**

The WireGuard public key IS the device identity (Cloudflare WARP model). No separate device ID.

```
~/.config/moto/
├── wg-private.key      # WireGuard private key (generated once)
└── wg-public.key       # WireGuard public key (this is your device identity)
```

- **Permissions:** Files MUST be 0600. CLI fails if permissions are wrong.
- **Generation:** On first `moto garage enter` if not exists
- **Override:** `MOTO_WG_KEY_FILE` env var to specify alternate location
- **Re-keying:** Generating a new keypair = new device identity, new IP assignment

**Garage WireGuard keys (ephemeral):**

- Generated when garage pod starts
- Registered with moto-club
- Stored in-memory only
- Destroyed when garage terminates

### Connection Flow

**`moto garage enter <name>`:**

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ 1. CLI checks local WG key exists                                            │
│    - If not: generate keypair, register with moto-club                       │
│    - POST /api/v1/wg/devices { public_key, device_name }                     │
│    - Retry with backoff (1s, 2s, 4s) on failure, then fail                   │
└──────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ 2. CLI requests tunnel session                                               │
│    - POST /api/v1/wg/sessions { garage_id, device_pubkey }                   │
│    - Server validates user owns garage and device                            │
│    - Garage discovers new peer on next poll (v1 uses REST polling)           │
│    - Returns: { garage_wg_pubkey, garage_ip, client_ip, derp_map }           │
│    - On moto-club unreachable: fail immediately, tell user to retry later    │
└──────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ 3. CLI configures WireGuard (userspace, no sudo)                             │
│    - Create virtual TUN (in-process abstraction)                             │
│    - Set local address = client_ip                                           │
│    - Add peer: garage_wg_pubkey, allowed_ips = garage_ip/128                 │
└──────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ 4. Connection establishment                                                  │
│    - Try direct UDP to garage endpoint (3 second timeout)                    │
│    - If direct fails: connect via DERP relay                                 │
│    - If DERP fails: try other DERP regions                                   │
│    - If all DERP regions fail: error to user                                 │
│    - WireGuard handshake completes                                           │
│    - Note: No upgrade attempts once on DERP (simplicity for v1)              │
└──────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ 5. WebSocket terminal over WireGuard tunnel                                  │
│    - Connect to ws://garage_ip:7681/                                         │
│    - ttyd serves terminal via WebSocket                                      │
│    - Attaches to tmux session (creates if not exists)                        │
│    - No authentication - tunnel establishment is the auth boundary           │
└──────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ 6. Interactive session                                                       │
│    - Terminal attached to tmux session in garage                             │
│    - Close connection or Ctrl+B, D to detach (tmux session stays active)     │
│    - Traffic flows directly (or via DERP), never through moto-club           │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Coordination API (moto-club)

The CLI and garage daemon coordinate with moto-club via REST APIs. See [moto-club.md](moto-club.md) for full API specifications.

**APIs used by CLI:**

| Endpoint | Purpose |
|----------|---------|
| `POST /api/v1/wg/devices` | Register device (first time only) |
| `POST /api/v1/wg/sessions` | Create tunnel session |
| `GET /api/v1/wg/sessions` | List active sessions |
| `DELETE /api/v1/wg/sessions/{id}` | Close session |

**APIs used by garage daemon:**

| Endpoint | Purpose |
|----------|---------|
| `POST /api/v1/wg/garages` | Register garage WireGuard endpoint |
| `GET /api/v1/wg/garages/{id}/peers` | Poll for authorized peers |

### Peer Streaming (WebSocket)

Garage maintains persistent WebSocket to moto-club for real-time peer updates:

```
WebSocket /internal/wg/garages/{garage_id}/peers
Authorization: Bearer <k8s-service-account-token>

// Server sends when client connects:
{ "action": "add", "public_key": "...", "allowed_ip": "fd00:moto:2::1/128" }

// Server sends when client disconnects:
{ "action": "remove", "public_key": "..." }

// Garage dynamically configures WireGuard peers based on these messages
```

**Heartbeat:** The WebSocket connection itself serves as heartbeat. If connection drops, moto-club knows garage is unavailable.

### Session Lifecycle

**Session ends when (whichever first):**
- Explicitly closed by user (DELETE /api/v1/wg/sessions/{id})
- Session TTL expires (CLI flag `--session-ttl`, defaults to garage TTL)
- Garage terminates

**Disconnect handling:**
- Client unexpectedly disconnects (laptop closes, network drops)
- Garage keeps peer configured for 5 minute grace period
- Client can reconnect within grace period without full re-registration
- After grace period, peer removed from garage WireGuard config

**Cleanup:**
- Background cron job cleans up expired sessions
- Sessions with terminated garages are immediately cleaned up

### DERP Relay

DERP (Designated Encrypted Relay for Packets) provides relay when direct P2P fails.

**How it works:**
1. Both peers connect to DERP server via HTTPS WebSocket
2. Traffic is already WireGuard-encrypted before reaching DERP
3. DERP forwards opaque encrypted packets
4. DERP never sees plaintext

**Deployment:** Self-hosted only. No external dependencies.

**Failover:**
- If primary DERP region fails, try other regions
- If all DERP servers fail, connection fails (report to user)

**DERP map structure:**

```rust
struct DerpMap {
    regions: HashMap<u16, DerpRegion>,
}

struct DerpRegion {
    region_id: u16,
    name: String,
    nodes: Vec<DerpNode>,
}

struct DerpNode {
    host: String,
    port: u16,        // DERP port (typically 443)
    stun_port: u16,   // STUN port (typically 3478)
}
```

**Protocol details:** Deferred. Implementation will use standard DERP protocol.

### MagicConn (Connection Multiplexer)

MagicConn handles direct UDP vs DERP multiplexing transparently.

```rust
pub enum PathType {
    Direct { endpoint: SocketAddr },
    Derp { region: String },
}

pub struct MagicConn {
    // Internal state
}

impl MagicConn {
    /// Send packet to peer, using best available path
    pub async fn send(&self, peer: &PublicKey, data: &[u8]) -> Result<()>;

    /// Receive packet from any peer
    pub async fn recv(&self) -> Result<(PublicKey, Vec<u8>)>;

    /// Get current path type for status display
    pub fn current_path(&self, peer: &PublicKey) -> Option<PathType>;
}
```

**Path selection (simple for v1):**
1. Try direct UDP (3 second timeout)
2. If direct fails, use DERP
3. No upgrade attempts once on DERP

### Terminal Daemon

**Garage runs ttyd with tmux:**

```
ttyd -p 7681 -W tmux new-session -A -s garage
```

| Setting | Value |
|---------|-------|
| Port | 7681 |
| WebSocket URL | `ws://[garage_ip]:7681/` |
| Shell | tmux (session name: "garage") |
| Working directory | `/workspace/<repo-name>/` (set by startup script after clone) |

**Session persistence:**
- First connect → creates tmux session, attaches
- Disconnect → tmux session keeps running (processes survive)
- Reconnect → reattaches to existing tmux session
- Detach: `Ctrl+B, D` or close connection

**Multiple connections:**
- Multiple clients can connect to the same garage
- All clients attach to the same tmux session (mirrored view)
- Use tmux windows/panes for separate workspaces

**Session cleanup:**
- When garage pod terminates, tmux session and all processes are killed
- No explicit cleanup needed - pod termination handles it

**Process management:**
- ttyd runs as systemd service
- Restarts on failure
- Health check: TCP probe on port 7681

**No authentication required** - tunnel establishment is the auth boundary.

### Security Model

**Authentication layers:**

| Layer | Mechanism | Purpose |
|-------|-----------|---------|
| moto-club API | User auth token | Authorize session creation |
| Garage registration | K8s namespace (pod can run = authorized) | Prove garage identity |
| WireGuard | Public key cryptography | Encrypt tunnel, **auth boundary** |

**No terminal-level authentication.** Tunnel establishment proves identity - if you can complete the WireGuard handshake, you're authorized to access the garage.

**Network isolation:**

```
AllowedIPs configuration ensures:
- Client can only reach its connected garages
- Garages cannot reach other garages via overlay
- No lateral movement possible
```

**What DERP relay sees:**
- Source and destination public keys
- Encrypted WireGuard packets (opaque blobs)
- Packet sizes and timing (metadata)
- Cannot decrypt, cannot inject, cannot MITM content

**Key security properties:**
- Forward secrecy (WireGuard's Noise protocol)
- No long-lived session keys on server
- Compromise of moto-club doesn't expose tunnel traffic
- Tunnel establishment is the sole auth boundary (no SSH keys needed)

### Error Handling

**Device registration (POST /api/v1/wg/devices):**
- Retry with exponential backoff: 1s, 2s, 4s (3 attempts)
- After 3 failures, report error to user

**Session creation (moto-club unreachable):**
- Fail immediately
- Report to user: "moto-club unreachable, please try again later"
- No automatic retry (user initiates retry)

**Direct connection:**
- 3 second timeout
- On timeout, silently fall back to DERP (user sees "Using DERP relay..." in output)
- If all DERP regions also fail, error is shown to user

**DERP connection:**
- 10 second timeout per region
- On failure, try next DERP region
- If all regions fail, report error to user

### Configuration

**Client config (`~/.config/moto/config.toml`):**

```toml
[wgtunnel]
# Prefer direct connections (default: true)
prefer_direct = true

# Connection timeouts
direct_timeout_secs = 3
derp_timeout_secs = 10

# WireGuard keepalive interval
keepalive_secs = 25
```

**Environment variables:**

```bash
# Override WireGuard key location
MOTO_WG_KEY_FILE="/path/to/wg-private.key"

# Force DERP only (skip direct attempts, for testing)
MOTO_WGTUNNEL_DERP_ONLY=1

# Debug logging
MOTO_WGTUNNEL_LOG=debug
```

**Default values:**

| Setting | Default |
|---------|---------|
| WireGuard keepalive | 25 seconds |
| Direct connection timeout | 3 seconds |
| DERP connection timeout | 10 seconds |
| Session TTL | Match garage TTL |
| Retry backoff | 1s, 2s, 4s (3 attempts) |
| Disconnect grace period | 5 minutes |

### CLI Commands

```bash
# Enter a garage (primary use case)
moto garage enter <name> [--session-ttl <duration>]
# Establishes tunnel, connects to terminal daemon
# --session-ttl: override session TTL (default: match garage TTL)

# Show tunnel status
moto tunnel status
# Lists active tunnels, shows path type (direct/DERP)

# Close tunnel explicitly
moto tunnel close <session-id>
```

**Enter flow output:**

```
$ moto garage enter feature-foo

Connecting to garage feature-foo...
  Creating session... done
  Configuring tunnel... done
  Attempting direct connection... timeout
  Using DERP relay (primary)... connected
  Connecting to terminal... done

root@feature-foo:/workspace/moto$
```

**Detach:** Close connection or `Ctrl+B, D` (tmux session stays active)

**Reattach:** `moto garage enter feature-foo` (reconnects to existing tmux session)

### Observability

**Logging:**
- Format: journald style (structured key=value)
- Default level: info
- Debug level via `MOTO_WGTUNNEL_LOG=debug`

**Health endpoint (garage daemon):**

```
GET /health

Response 200:
{
  "status": "healthy",
  "wireguard": "up",
  "moto_club_connected": true,
  "active_peers": 2
}
```

**Metrics:** Deferred to future work.

### Platform Support

| Platform | Support | Notes |
|----------|---------|-------|
| Linux | Full | Platform-specific TUN abstraction |
| macOS | Full | Platform-specific TUN abstraction |
| Windows | Not supported | May be added in future |

**Implementation:** Platform-specific APIs used directly for TUN abstraction. No dependency on unmaintained crates.

## Implementation Notes

**Dependencies:**
- `boringtun` - Userspace WireGuard implementation
- `x25519-dalek` - Key generation
- `tokio` - Async runtime
- Platform-specific TUN APIs (no external TUN crate)

**Testing:**
- Unit tests with mock DERP
- Integration tests with local WireGuard
- E2E tests require K8s cluster

### Implementation Status

**Complete (standalone crates):**

| Crate | Status | Notes |
|-------|--------|-------|
| moto-wgtunnel-types | ✓ Complete | Keys, IPs, peers, DERP types |
| moto-wgtunnel-derp | ✓ Complete | Protocol, client, map |
| moto-wgtunnel-conn | ✓ Complete | MagicConn, STUN, endpoint selection |
| moto-wgtunnel-engine | ✓ Complete | Tunnel management, platform TUN |
| moto-club-wg | ✓ Complete | IPAM, peers, sessions, DERP |
| moto-garage-wgtunnel | ✓ Complete | Daemon with WS peer streaming, WireGuard engine integration, reconnect, health |
| moto-cli-wgtunnel | ✓ Complete | WireGuard engine, direct UDP, DERP relay, ttyd terminal all wired up |

### Garage Daemon WebSocket Client (Remaining)

The daemon has `peer_stream_url()` and `handle_peer_action()` scaffolded but `run()` is a placeholder. The daemon needs to:

1. **Connect to peer streaming WebSocket** at `peer_stream_url()` with `Authorization: Bearer <k8s-sa-token>`
   - Read token from `/var/run/secrets/kubernetes.io/serviceaccount/token`
   - Convert moto-club base URL to WebSocket scheme (`http→ws`, `https→wss`)

2. **Process incoming PeerEvents** — parse JSON messages from the WebSocket:
   - `{"action": "add", "public_key": "...", "allowed_ip": "fd00:moto:2::1/128"}` → configure WireGuard peer
   - `{"action": "remove", "public_key": "..."}` → remove WireGuard peer
   - Use `moto-wgtunnel-engine` to actually add/remove peers on the WireGuard interface

3. **Reconnect on disconnect** — if the WebSocket drops:
   - Exponential backoff: 1s, 2s, 4s, 8s, capped at 30s
   - On reconnect, server sends full peer list as `add` events (re-sync)
   - Log warning on each reconnect attempt

4. **Event loop in `run()`** — the complete daemon loop:
   a. Call `register()` to register garage WireGuard endpoint with moto-club
   b. Spawn health endpoint HTTP server (existing `health.rs`)
   c. Initialize WireGuard tunnel via `moto-wgtunnel-engine`
   d. Connect to peer streaming WebSocket
   e. Loop: receive PeerEvents, configure WireGuard peers, handle Ping/Pong
   f. On shutdown signal (SIGTERM): close WebSocket, clean up tunnel

5. **Replace placeholders** in `handle_peer_action()`:
   - `PeerAction::Add` → call engine to add peer with public_key and allowed_ip
   - `PeerAction::Remove` → call engine to remove peer by public_key

6. **Health endpoint integration** — update health state based on:
   - WebSocket connection status (`moto_club_connected: true/false`)
   - WireGuard tunnel status (`wireguard: "up"/"down"`)
   - Active peer count

## Changelog

### v0.10 (2026-03-06)
- Fix: moto-garage-wgtunnel status corrected from Complete to Partial — `run()` is a placeholder, `handle_peer_action()` has stub implementations
- Add: Garage Daemon WebSocket Client section specifying what `run()` must do: connect to peer streaming WebSocket, process PeerEvents, configure WireGuard via engine, reconnect with backoff, health integration
- Reference: Peer streaming protocol defined in [moto-club-websocket.md](moto-club-websocket.md)

### v0.9
- Update implementation status: moto-cli-wgtunnel is Complete (was incorrectly marked Partial)
- Remove "Remaining integration" and "Blocked" sections (all tasks complete)
- All 7 crates fully implemented and functional

### v0.8
- Replace SSH with ttyd + tmux for terminal access
- Remove SSH key management (no ssh_keys.rs, no SSH key endpoints)
- Update connection flow: WebSocket to ttyd instead of SSH
- Add Terminal Daemon section (port 7681, session persistence, multiple connections)
- Update security model: tunnel establishment is the sole auth boundary
- Add process management details (systemd, health check)

### v0.7
- Simplified device identity: WireGuard public key IS the device identifier (Cloudflare WARP model)
- Removed `device_id` concept and `~/.config/moto/device-id` file
- Updated connection flow to use `device_pubkey` instead of `device_id`

### v0.6
- Removed duplicated API specifications; reference moto-club.md for API contracts
- Updated blocking section to reflect spec completion (implementation still needed)

### v0.5
- Fix: `POST /api/v1/wg/devices` must accept `device_id` from client (not server-generated)

### v0.4
- Initial specification

## References

- [boringtun](https://github.com/cloudflare/boringtun) - Userspace WireGuard
- [WireGuard Protocol](https://www.wireguard.com/protocol/) - Noise protocol details
- [STUN RFC 5389](https://tools.ietf.org/html/rfc5389) - NAT discovery
