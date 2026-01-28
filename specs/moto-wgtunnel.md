# WireGuard Tunnel System

| | |
|--------|----------------------------------------------|
| Version | 0.4 |
| Last Updated | 2026-01-22 |

## Overview

Secure connectivity layer for terminal/SSH access to garages. WireGuard provides encrypted P2P tunnels with DERP relay fallback for NAT traversal. moto-club coordinates peer discovery and IP allocation but never sees traffic.

**This is for terminal access only.** Streaming logs, TTL warnings, and events use WebSocket/SSE (see moto-club.md).

**Key properties:**
- Real SSH sessions over encrypted WireGuard tunnel
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
│  │  SSH Server (accepts connections from tunnel, user's key injected)   │   │
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
│  │  ├── User SSH Key Storage (injected into garages)                    │    │
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
│       ├── ssh_keys.rs         # User SSH key management
│       └── derp.rs             # DERP map management
│
└── moto-garage-wgtunnel/       # Garage-side daemon
    └── src/
        ├── lib.rs
        ├── daemon.rs           # Main daemon loop
        ├── register.rs         # Register with moto-club
        ├── health.rs           # Health endpoint
        └── ssh.rs              # SSH server integration
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
- **Clients:** Allocated per device, persisted. Same device re-registering gets same IP.
- **Device ID:** Random UUID, generated on first use

**Why IPv6 ULA:**
- No collision with public IPs
- Large address space (no exhaustion concerns)
- Standard prefix recognized by networking tools

### Key Management

**Client WireGuard keys (persistent):**

```
~/.config/moto/
├── wg-private.key      # WireGuard private key (generated once)
├── wg-public.key       # WireGuard public key
└── device-id           # UUID, unique device identifier
```

- **Permissions:** Files MUST be 0600. CLI fails if permissions are wrong.
- **Generation:** On first `moto garage enter` if not exists
- **Override:** `MOTO_WG_KEY_FILE` env var to specify alternate location

**Client SSH keys (standard location):**

```
~/.ssh/
├── id_ed25519          # User's SSH private key
└── id_ed25519.pub      # User's SSH public key (registered with moto-club)
```

- **Scope:** Per-user (same key for all devices, all garages)
- **Registration:** User registers SSH public key with moto-club
- **Injection:** moto-club injects user's public key into garage's `authorized_keys` at creation

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
│    - POST /api/v1/wg/devices { device_id, wg_public_key }                    │
│    - Retry with backoff (1s, 2s, 4s) on failure, then fail                   │
└──────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ 2. CLI requests tunnel session                                               │
│    - POST /api/v1/wg/sessions { garage_id, device_id }                       │
│    - Server validates user owns garage                                       │
│    - Server notifies garage of new peer via WebSocket                        │
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
│ 5. SSH over WireGuard tunnel                                                 │
│    - Connect to garage_ip:22                                                 │
│    - Authenticate with user's SSH key (from ~/.ssh/)                         │
│    - Key was injected into garage by moto-club at creation                   │
└──────────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│ 6. Interactive session                                                       │
│    - TTY attached to garage shell                                            │
│    - Ctrl+P, Ctrl+Q to detach (session stays active)                         │
│    - Traffic flows directly (or via DERP), never through moto-club           │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Coordination API (moto-club)

**Register device (first time):**

```
POST /api/v1/wg/devices
Authorization: Bearer <user-token>

{
  "device_id": "uuid-of-device",
  "public_key": "base64-encoded-wg-public-key",
  "device_name": "macbook-pro"  // optional, for display
}

Response 201:
{
  "device_id": "uuid-of-device",
  "assigned_ip": "fd00:moto:2::1"
}

Response 409 (already registered):
{
  "device_id": "uuid-of-device",
  "assigned_ip": "fd00:moto:2::1"  // returns existing IP
}
```

**Register user SSH key:**

```
POST /api/v1/users/ssh-keys
Authorization: Bearer <user-token>

{
  "public_key": "ssh-ed25519 AAAA... user@host"
}

Response 201:
{
  "fingerprint": "SHA256:..."
}
```

**Create tunnel session:**

```
POST /api/v1/wg/sessions
Authorization: Bearer <user-token>

{
  "garage_id": "abc123",
  "device_id": "uuid-of-device",
  "ttl_seconds": 14400  // optional, defaults to garage TTL
}

Response 201:
{
  "session_id": "sess_xyz789",
  "garage": {
    "public_key": "base64-encoded-garage-wg-public-key",
    "overlay_ip": "fd00:moto:1::abc1",
    "endpoints": [
      "203.0.113.5:51820"
    ]
  },
  "client_ip": "fd00:moto:2::1",
  "derp_map": {
    "regions": {
      "1": {
        "name": "primary",
        "nodes": [
          { "host": "derp.example.com", "port": 443, "stun_port": 3478 }
        ]
      }
    }
  },
  "expires_at": "2026-01-21T16:00:00Z"
}
```

**Garage registration (called by garage pod on startup):**

```
POST /api/v1/wg/garages
Authorization: Bearer <k8s-service-account-token>

{
  "garage_id": "abc123",
  "public_key": "base64-encoded-garage-wg-public-key",
  "endpoints": ["10.42.0.5:51820"]
}

Response 200:
{
  "assigned_ip": "fd00:moto:1::abc1",
  "derp_map": { ... }
}
```

Note: Garage registration trusts the pod. If a pod can run in the garage namespace, it's authorized.

**List active sessions:**

```
GET /api/v1/wg/sessions
Authorization: Bearer <user-token>

Response 200:
{
  "sessions": [
    {
      "session_id": "sess_xyz789",
      "garage_id": "abc123",
      "garage_name": "feature-foo",
      "device_id": "uuid-of-device",
      "created_at": "2026-01-21T12:00:00Z",
      "expires_at": "2026-01-21T16:00:00Z"
    }
  ]
}
```

**Close session:**

```
DELETE /api/v1/wg/sessions/{session_id}
Authorization: Bearer <user-token>

Response 204 No Content
```

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

### SSH Configuration

**Garage SSH server:**

```
Port 22
ListenAddress <overlay_ip>
PubkeyAuthentication yes
PasswordAuthentication no
AuthorizedKeysFile /home/moto/.ssh/authorized_keys
```

- **User:** `moto` (non-root user in garage)
- **Shell:** `/bin/bash` in workspace directory
- **Key injection:** moto-club writes user's SSH public key to `authorized_keys` when garage is created

**Client SSH:**
- Uses standard `~/.ssh/id_ed25519` (or other key types)
- User registers public key with moto-club once
- Same key works for all garages owned by user

### Security Model

**Authentication layers:**

| Layer | Mechanism | Purpose |
|-------|-----------|---------|
| moto-club API | User auth token | Authorize session creation |
| Garage registration | K8s namespace (pod can run = authorized) | Prove garage identity |
| WireGuard | Public key cryptography | Encrypt tunnel |
| SSH | Public key auth (injected by moto-club) | Authenticate user to shell |

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
- Tunnel establishment is the auth boundary

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
- On timeout, fall back to DERP (no error to user, just slower path)

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
# Establishes tunnel, opens SSH session
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
  Opening SSH session... done

moto@feature-foo:/workspace$
```

**Detach:** `Ctrl+P, Ctrl+Q` (session stays active)

**Reattach:** `moto garage enter feature-foo` (reconnects to existing session)

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
| moto-club-wg | ✓ Complete | IPAM, peers, sessions, SSH keys, DERP |
| moto-garage-wgtunnel | ✓ Complete | Daemon, registration, health, SSH |
| moto-cli-wgtunnel | Partial | Types complete, integration pending |

**Remaining integration (can implement now):**

| Task | Location | Description |
|------|----------|-------------|
| Wire up WireGuard engine | `enter.rs` | Connect `moto-wgtunnel-engine` to configure tunnel |
| Wire up direct UDP | `enter.rs` | Use `MagicConn` for direct connection attempts |
| Wire up DERP relay | `enter.rs` | Use `DerpClient` for relay fallback |
| SSH session spawning | `enter.rs` | Exec SSH to garage overlay IP after tunnel up |

**Blocked on moto-club.md:**

| Task | Blocked By | Description |
|------|------------|-------------|
| Device registration | `POST /api/v1/wg/devices` | moto-club must expose endpoint |
| Session creation | `POST /api/v1/wg/sessions` | moto-club must expose endpoint |
| Garage peer info | `POST /api/v1/wg/garages` | moto-club must expose endpoint |

The types and logic for these APIs exist in `moto-club-wg`. The moto-club server needs to wire up HTTP handlers that use these crates. See moto-club.md TODO (lines 12-22) for the coordination API work.

## References

- [boringtun](https://github.com/cloudflare/boringtun) - Userspace WireGuard
- [WireGuard Protocol](https://www.wireguard.com/protocol/) - Noise protocol details
- [STUN RFC 5389](https://tools.ietf.org/html/rfc5389) - NAT discovery
