# Moto Club

| | |
|--------|----------------------------------------------|
| Version | 1.5 |
| Status | Ripping |
| Last Updated | 2026-02-04 |

## Overview

The motorcycle club - central orchestration service for the moto platform. Where the riders gather and garages are managed. Handles garage lifecycles, WireGuard coordination for remote access, and K8s coordination.

**Scope for v1:**
- Garage management (create, track, close)
- WireGuard coordination (peer registration, IP allocation)
- K8s namespace/pod management
- Poll-based reconciliation (K8s → database)
- Supporting services per garage (Postgres, Redis) - see [supporting-services.md](supporting-services.md)

**Deferred:**
- Bike management (deploy, track, stop) - future version
- TTL enforcement loop (handled by moto-cron) - manual `garage close` for now
- Authentication/identity system
- WebSocket streaming (REST polling for v1) - see [moto-club-websocket.md](moto-club-websocket.md)
- Peer streaming endpoint details

**Dependencies:**
- [dev-container.md](dev-container.md) - Garage container image (blocks garage creation)

**Design principles:**
- K8s pods are source of truth, database is historical record
- Single instance for v1, designed for HA later
- Multi-user data model (owner field), but no auth for local dev
- Local dev should mirror prod as closely as possible
- Degrade gracefully on dependency failures

## Specification

### Responsibilities

| Domain | Responsibility |
|--------|----------------|
| **Garages** | Create, track, close wrenching environments |
| **WireGuard** | Coordinate peer registration, IP allocation, DERP map |
| **K8s** | Create namespaces, deploy pods, manage resources |
| **Reconciliation** | Poll K8s, sync state to database |
| **Registry** | Track all garages (current and historical) |

### Architecture

```
                                    ┌─────────────────────────────────────┐
                                    │            moto-club                │
                                    │                                     │
┌──────────────┐    HTTP/WS         │  ┌───────────┐    ┌───────────┐    │
│   moto CLI   │ ◀────────────────▶ │  │  REST API │    │ WebSocket │    │
└──────────────┘                    │  │  Handler  │    │  Handler  │    │
                                    │  └───────────┘    └───────────┘    │
                                    │        │               │           │
                                    │        ▼               ▼           │
                                    │  ┌─────────────────────────────┐   │
                                    │  │      Core Services          │   │
                                    │  │                             │   │
                                    │  │  - GarageService            │   │
                                    │  │  - WireGuardCoordinator     │   │
                                    │  │  - K8sClient                │   │
                                    │  │  - Reconciler               │   │
                                    │  └─────────────────────────────┘   │
                                    │        │                           │
                                    └────────┼───────────────────────────┘
                                             │
              ┌──────────────────────────────┼──────────────────────────────┐
              │                              │                              │
              ▼                              ▼                              ▼
       ┌──────────┐                   ┌──────────┐                   ┌──────────┐
       │ Postgres │                   │   K8s    │                   │  Keybox  │
       │ (state)  │                   │ (pods)   │                   │(secrets) │
       └──────────┘                   └──────────┘                   └──────────┘
```

### Connectivity Model

Two transports for different purposes:

1. **WireGuard** - Terminal access to garages (via ttyd)
   - CLI establishes WireGuard tunnel directly to garage pod
   - moto-club coordinates only (peer registration, IP allocation)
   - Traffic never flows through moto-club
   - See [moto-wgtunnel.md](moto-wgtunnel.md) for details

2. **WebSocket** - Streaming logs, real-time events
   - CLI connects to moto-club WebSocket endpoints
   - Used for log streaming, TTL warnings, status changes
   - See [moto-club-websocket.md](moto-club-websocket.md) for details

```
Terminal access (WireGuard):
┌──────────────┐     WireGuard      ┌──────────────┐
│   moto CLI   │ ◀────────────────▶ │  Garage Pod  │
│   (local)    │   tunnel + ttyd    │   (remote)   │
└──────────────┘                    └──────────────┘
        │
        │  coordinate (HTTP)
        ▼
┌──────────────┐
│  moto-club   │  (peer registration, IP allocation only)
└──────────────┘

Streaming (WebSocket):
┌──────────────┐     WebSocket      ┌──────────────┐
│   moto CLI   │ ◀────────────────▶ │  moto-club   │
└──────────────┘   logs, events     └──────────────┘
```

### Crate Structure

```
crates/
├── moto-club/                # Binary: composes all crates, runs the server
│   └── src/
│       └── main.rs
│
├── moto-club-api/            # Library: REST API handlers
│   └── src/
│       ├── lib.rs
│       ├── garages.rs        # Garage endpoints
│       ├── wg.rs             # WireGuard coordination endpoints
│       └── health.rs         # Health/info endpoints
│
├── moto-club-ws/             # Library: WebSocket handlers
│   └── src/
│       ├── lib.rs
│       ├── logs.rs           # Log streaming
│       └── events.rs         # Real-time events
│
├── moto-club-wg/             # Library: WireGuard coordination
│   └── src/
│       ├── lib.rs
│       ├── peers.rs          # Peer registration
│       ├── ipam.rs           # IP address allocation
│       ├── sessions.rs       # Tunnel session management
│       └── derp.rs           # DERP map management
│
├── moto-club-garage/         # Library: Garage service logic
│   └── src/
│       ├── lib.rs
│       ├── service.rs        # GarageService
│       └── lifecycle.rs      # State transitions
│
├── moto-club-k8s/            # Library: Kubernetes interactions
│   └── src/
│       ├── lib.rs
│       ├── namespace.rs      # Namespace management
│       ├── pods.rs           # Pod lifecycle
│       └── resources.rs      # Quotas, policies
│
├── moto-club-db/             # Library: Database layer
│   └── src/
│       ├── lib.rs
│       ├── models.rs         # Data models
│       └── garage_repo.rs    # Garage repository
│
├── moto-club-reconcile/      # Library: K8s → DB reconciliation
│   └── src/
│       ├── lib.rs
│       └── garage.rs         # Garage reconciler
│
└── moto-club-types/          # Library: Shared types (used by CLI too)
    └── src/
        ├── lib.rs
        ├── garage.rs         # Garage types
        ├── error.rs          # Error types
        └── api.rs            # Request/response types
```

**Test Organization:** Tests for large modules should be in separate files per AGENTS.md convention:
- `moto-club-api/src/wg.rs` → tests in `moto-club-api/src/wg_test.rs`
- `moto-club-k8s/src/pods.rs` → tests in `moto-club-k8s/src/pods_test.rs`

**Deferred crates (future):**
- `moto-club-bike/` - Bike service logic
- `moto-club-bike-repo/` - Bike repository

**Dependency graph:**

```
                    moto-club (binary)
                         │
         ┌───────────────┼───────────────┐
         │               │               │
         ▼               ▼               ▼
   moto-club-api   moto-club-ws   (config, etc.)
         │               │
         └───────┬───────┘
                 │
    ┌────────────┼────────────┬────────────┐
    │            │            │            │
    ▼            ▼            ▼            ▼
moto-club-   moto-club-   moto-club-   moto-club-
  garage       wg         reconcile      db
    │            │            │            │
    └─────┬──────┴────────────┘            │
          │                                │
          ▼                                │
    moto-club-k8s                          │
          │                                │
          └────────────┬───────────────────┘
                       │
                       ▼
                moto-club-types
```

### Owner Identity (Local Dev)

Since authentication is deferred, owner identity comes from config:

**Config file (`~/.config/moto/config.toml`):**
```toml
[user]
name = "nick"
```

**Environment override:**
```bash
MOTO_USER="nick"
```

**Precedence:** Environment > Config file > Error (must be set)

**API Authentication (Local Dev):**

The CLI sends owner identity as a fake Bearer token:

```
Authorization: Bearer nick
```

The token value IS the username. moto-club extracts the owner directly from the token. This mirrors the real auth flow (Bearer token in header) while keeping local dev simple.

When the identity system is implemented, this will be replaced with proper JWT tokens that moto-club validates.

### REST API Endpoints

#### Garage Management

##### Create Garage

```
POST /api/v1/garages
Authorization: Bearer <user-token>

Request:
{
  "name": "my-feature",              // Optional, auto-generated if omitted (e.g., "bold-mongoose")
  "branch": "feature/foo",           // Optional, CLI determines from local repo if omitted
  "ttl_seconds": 14400,              // Optional, default from MOTO_CLUB_DEFAULT_TTL_SECONDS
  "image": "ghcr.io/custom:v1"       // Optional, override dev container image
}

Response 201 Created:
{
  "id": "uuid-of-garage",
  "name": "bold-mongoose",
  "owner": "nick",
  "branch": "feature/foo",
  "status": "pending",
  "image": "ghcr.io/nhalm/moto-dev:latest",
  "ttl_seconds": 14400,
  "expires_at": "2026-01-28T20:00:00Z",
  "created_at": "2026-01-28T16:00:00Z",
  "updated_at": "2026-01-28T16:00:00Z",
  "namespace": "moto-garage-uuid-of-garage",
  "pod_name": "garage"
}
```

**Behavior:**
- Name is immutable once created
- Owner extracted from Bearer token
- Returns immediately with `status: pending`; pod creation is async

**Name generation (if not provided):**
- Format: `{adjective}-{animal}` (e.g., "bold-mongoose", "swift-falcon")
- Max length: 63 characters (K8s label limit)
- Allowed characters: lowercase alphanumeric and hyphens, must start/end with alphanumeric
- On collision: append random 4-digit suffix (e.g., "bold-mongoose-7x2k")
- Retry up to 3 times on collision, then fail with `INTERNAL_ERROR`

**Errors:**
- `GARAGE_ALREADY_EXISTS` (409) - Name already taken
- `INVALID_TTL` (400) - TTL below minimum (60s) or above maximum (48h default)

##### List Garages

```
GET /api/v1/garages
Authorization: Bearer <user-token>

Query Parameters:
  ?status=initializing,ready  // Optional, filter by status (comma-separated)
                              // Valid: pending, initializing, ready, failed, terminated
  ?all=true                // Optional, include terminated garages (default: false)

Response 200:
{
  "garages": [
    {
      "id": "uuid-of-garage",
      "name": "bold-mongoose",
      "owner": "nick",
      "branch": "feature/foo",
      "status": "ready",
      "ttl_seconds": 14400,
      "expires_at": "2026-01-28T20:00:00Z",
      "created_at": "2026-01-28T16:00:00Z"
    }
  ]
}
```

**Behavior:**
- Automatically filtered to requesting user's garages (from Bearer token)
- By default excludes terminated garages
- No pagination for v1 (user unlikely to have many active garages)
- Invalid status values return `INVALID_STATUS` (400) error

**Errors:** `INVALID_STATUS` (400) - Unknown status value in filter

##### Get Garage

```
GET /api/v1/garages/{name}
Authorization: Bearer <user-token>

Response 200:
{
  "id": "uuid-of-garage",
  "name": "bold-mongoose",
  "owner": "nick",
  "branch": "feature/foo",
  "status": "ready",
  "image": "ghcr.io/nhalm/moto-dev:latest",
  "ttl_seconds": 14400,
  "expires_at": "2026-01-28T20:00:00Z",
  "created_at": "2026-01-28T16:00:00Z",
  "updated_at": "2026-01-28T16:05:00Z",
  "namespace": "moto-garage-uuid-of-garage",
  "pod_name": "garage",
  "terminated_at": null,
  "termination_reason": null
}
```

**Errors:** `GARAGE_NOT_FOUND` (404), `GARAGE_NOT_OWNED` (403)

##### Delete Garage

```
DELETE /api/v1/garages/{name}
Authorization: Bearer <user-token>

Response 204 No Content
```

**Behavior:**
- Works on any garage status (Pending, Initializing, Ready, Failed)
- Sets status to `terminated`
- Sets `terminated_at` timestamp
- Sets `termination_reason` to `user_closed`
- Deletes K8s namespace (cascades to all resources)
- Idempotent: deleting already-terminated garage returns 204

**Errors:** `GARAGE_NOT_FOUND` (404), `GARAGE_NOT_OWNED` (403)

##### Extend Garage TTL

```
POST /api/v1/garages/{name}/extend
Authorization: Bearer <user-token>

Request:
{
  "seconds": 7200                    // Seconds to ADD to current expiry
}

Response 200:
{
  "expires_at": "2026-01-28T22:00:00Z",
  "ttl_remaining_seconds": 21600
}
```

**Behavior:**
- Adds seconds to current `expires_at` (not to original TTL)
- Total TTL = `new_expires_at - created_at`. Cannot exceed `MOTO_CLUB_MAX_TTL_SECONDS`.
- Example: garage created 2h ago with 4h TTL (2h remaining). Extend by 1h. New expiry = now + 3h. Total TTL = 5h.

**Errors:**
- `GARAGE_NOT_FOUND` (404)
- `GARAGE_NOT_OWNED` (403)
- `GARAGE_EXPIRED` (410) - Cannot extend expired garage
- `GARAGE_TERMINATED` (410) - Cannot extend terminated garage
- `INVALID_TTL` (400) - Would exceed max TTL or below minimum (1 minute)

#### WireGuard Coordination

##### Register Client Device

Registers a user's device for WireGuard tunnel access. Called on first `moto garage enter` if device not yet registered.

The WireGuard public key IS the device identity (Cloudflare WARP model). No separate device ID.

```
POST /api/v1/wg/devices
Authorization: Bearer <user-token>

Request:
{
  "public_key": "base64-wg-public-key",    // WireGuard public key (device identity)
  "device_name": "macbook-pro"             // Optional, for display
}

Response 201 Created:
{
  "public_key": "base64-wg-public-key",
  "assigned_ip": "fd00:moto:2::1"          // IPv6 overlay address
}

Response 200 OK (already registered, idempotent):
{
  "public_key": "base64-wg-public-key",
  "assigned_ip": "fd00:moto:2::1"          // Returns existing IP
}
```

**Behavior:**
- WireGuard public key is the device identifier
- Same key re-registering returns existing assignment (idempotent)
- Re-keying (new WG keypair) = new device registration, new IP

**Errors:** `DEVICE_NOT_OWNED` (403) if public key registered to different user

##### Get Device Info

```
GET /api/v1/wg/devices/{public_key}
Authorization: Bearer <user-token>

Response 200:
{
  "public_key": "base64-wg-public-key",
  "device_name": "macbook-pro",
  "assigned_ip": "fd00:moto:2::1",
  "created_at": "2026-01-21T10:00:00Z"
}
```

**Note:** Public key must be URL-encoded in the path.

**Errors:** `DEVICE_NOT_FOUND` (404), `DEVICE_NOT_OWNED` (403)

##### Create Tunnel Session

Creates a tunnel session authorizing a device to connect to a garage.

```
POST /api/v1/wg/sessions
Authorization: Bearer <user-token>

Request:
{
  "garage_id": "uuid-of-garage",
  "device_pubkey": "base64-client-wg-public-key",
  "ttl_seconds": 14400                     // Optional, defaults to garage TTL
}

Response 201 Created:
{
  "session_id": "sess_xyz789",
  "garage": {
    "public_key": "base64-garage-wg-public-key",
    "overlay_ip": "fd00:moto:1::abc1",
    "endpoints": ["203.0.113.5:51820"]     // Direct UDP endpoints to try
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

**Behavior:**
- Validates user owns the garage and the device
- Increments `wg_garages.peer_version` for the target garage
- Garage discovers new peer on next poll of `GET /api/v1/wg/garages/{id}/peers`
- Session TTL: defaults to garage's remaining TTL. If requested TTL exceeds garage remaining TTL, it's capped (session can't outlive garage).
- **TTL capping detection:** Response `expires_at` shows actual expiry. Client can compare with requested `ttl_seconds` to detect if TTL was capped.
- Returns all info needed for client to establish tunnel

**Errors:**
- `GARAGE_NOT_FOUND` (404) - Garage doesn't exist
- `GARAGE_NOT_OWNED` (403) - User doesn't own garage
- `GARAGE_EXPIRED` (410) - Garage TTL has expired (even if not yet marked terminated)
- `GARAGE_TERMINATED` (410) - Garage has been terminated
- `GARAGE_NOT_REGISTERED` (400) - Garage hasn't registered its WireGuard endpoint yet
- `DEVICE_NOT_FOUND` (404) - Device not registered
- `DEVICE_NOT_OWNED` (403) - Device belongs to different user

##### List Active Sessions

```
GET /api/v1/wg/sessions
Authorization: Bearer <user-token>

Query Parameters:
  ?garage_id=uuid          // Optional, filter by garage
  ?all=true                // Optional, include expired/closed sessions (default: false)

Response 200:
{
  "sessions": [
    {
      "session_id": "sess_xyz789",
      "garage_id": "uuid-of-garage",
      "garage_name": "bold-mongoose",
      "device_pubkey": "base64-client-wg-public-key",
      "device_name": "macbook-pro",
      "created_at": "2026-01-21T12:00:00Z",
      "expires_at": "2026-01-21T16:00:00Z"
    }
  ]
}
```

**Behavior:**
- Automatically filtered to requesting user's sessions
- By default excludes expired sessions (`expires_at < now`) and closed sessions (`closed_at IS NOT NULL`)
- `?all=true` includes expired and closed sessions

##### Close Session

```
DELETE /api/v1/wg/sessions/{session_id}
Authorization: Bearer <user-token>

Response 204 No Content
```

**Behavior:**
- Soft delete: sets `closed_at` timestamp on session record (row preserved for audit)
- Increments `wg_garages.peer_version` for the session's garage
- Garage removes peer on next poll (detects via version change)
- Idempotent: closing already-closed session returns 204

**Errors:** `SESSION_NOT_FOUND` (404), `SESSION_NOT_OWNED` (403)

##### Register Garage WireGuard

Called by garage pod on startup to register its WireGuard endpoint.

```
POST /api/v1/wg/garages
Authorization: Bearer <k8s-service-account-token>

Request:
{
  "garage_id": "uuid-of-garage",
  "public_key": "base64-garage-wg-public-key",
  "endpoints": ["10.42.0.5:51820"]         // Pod's reachable endpoints
}

Response 200:
{
  "assigned_ip": "fd00:moto:1::abc1",      // Garage's overlay IP
  "derp_map": {
    "regions": { ... }
  }
}
```

**Validation:**
1. K8s ServiceAccount token validated via TokenReview API
2. Pod must be in namespace `moto-garage-{garage_id}`
3. Prevents rogue pods from registering as arbitrary garages

**Behavior:**
- If garage already registered, updates public_key and endpoints (garage pods generate new keypair on restart)
- Assigned IP is deterministic (derived from garage_id hash), stays same across re-registration
- Active sessions remain valid (clients will reconnect with new garage pubkey on next session create)

**Security note:** Re-registration with different pubkey is allowed because:
1. K8s namespace validation ensures only pods in `moto-garage-{id}` namespace can register
2. Garage pods generate ephemeral keypairs on startup (no persistent key)
3. If pod can run in the namespace, it's authorized (namespace = trust boundary)

**Errors:**
- `GARAGE_NOT_FOUND` (404) - Garage doesn't exist
- `INVALID_TOKEN` (401) - K8s ServiceAccount token invalid or expired
- `NAMESPACE_MISMATCH` (403) - Pod not running in expected garage namespace

##### Get Garage WireGuard Registration

Retrieves garage's WireGuard registration (for restart recovery or status check).

```
GET /api/v1/wg/garages/{garage_id}
Authorization: Bearer <k8s-service-account-token>

Response 200:
{
  "garage_id": "uuid-of-garage",
  "public_key": "base64-garage-wg-public-key",
  "assigned_ip": "fd00:moto:1::abc1",
  "endpoints": ["10.42.0.5:51820"],
  "peer_version": 42,
  "derp_map": {
    "regions": { ... }
  },
  "registered_at": "2026-01-28T16:00:00Z"
}

Response 404 (not registered):
// Garage exists but hasn't registered WireGuard yet
```

**Behavior:**
- Returns current registration including latest DERP map
- Useful after garage pod restart to recover state
- Same validation as POST (K8s token, namespace match)

**Errors:**
- `GARAGE_NOT_FOUND` (404) - Garage doesn't exist or not registered
- `INVALID_TOKEN` (401) - K8s ServiceAccount token invalid
- `NAMESPACE_MISMATCH` (403) - Pod not in expected namespace

##### Get Garage Peers (Polling)

Garage polls this endpoint to get current authorized peers.

```
GET /api/v1/wg/garages/{garage_id}/peers
Authorization: Bearer <k8s-service-account-token>

Query Parameters:
  ?version=41              // Optional, return 304 if current version equals this

Response 200 (peers changed or no version param):
{
  "peers": [
    {
      "public_key": "base64-client-wg-public-key",
      "allowed_ip": "fd00:moto:2::1/128"
    }
  ],
  "version": 42
}

Response 304 Not Modified (version matches, no body):
// Returned when ?version=42 and current version is still 42
```

**Behavior:**
- Returns all active sessions for this garage
- Garage configures WireGuard peers based on this list
- Poll interval: 5 seconds (garage-side config)
- **Conditional GET:** Pass `?version=N` to get 304 if nothing changed (reduces bandwidth)
- `version` field: monotonic counter in `wg_garages.peer_version`, incremented on session create/close

**Recommended polling pattern:**
1. First poll: `GET /peers` (no version param) → get initial peers + version
2. Subsequent polls: `GET /peers?version=42` → 304 if unchanged, 200 with new data if changed

**Note:** WebSocket streaming will replace polling in a future version.

#### DERP Map

##### Get DERP Map

Returns current DERP server map. Clients and garages can poll this to detect DERP server changes.

```
GET /api/v1/wg/derp-map
Authorization: Bearer <user-token> OR <k8s-service-account-token>

Response 200:
{
  "regions": {
    "1": {
      "name": "primary",
      "nodes": [
        { "host": "derp.example.com", "port": 443, "stun_port": 3478 }
      ]
    }
  },
  "version": 5                             // Incremented when DERP config changes
}
```

**Behavior:**
- Returns only healthy DERP servers (`healthy = true`)
- `version` increments when DERP servers are added/removed/change health status
- Clients/garages can poll periodically (recommended: every 5 minutes) to detect changes
- Initial DERP map is provided during session/garage registration; this endpoint is for updates

#### Health/Info

##### Health Check

```
GET /health
```

See [Health Check](#health-check) section for response format.

##### Server Info

```
GET /api/v1/info

Response 200:
{
  "name": "moto-club",
  "version": "0.1.0",
  "api_version": "v1",
  "git_sha": "abc1234",              // Build git commit (if available)
  "features": {
    "websocket": false,              // WebSocket streaming enabled
    "derp_regions": 1                // Number of DERP regions available
  }
}
```

No authentication required.

#### WebSocket Endpoints

**Peer streaming (implemented):**

```
WS /internal/wg/garages/{id}/peers
```

Real-time peer updates for garage pods. Authenticated via K8s ServiceAccount token. Broadcasts peer add/remove events.

For clients preferring polling, `GET /api/v1/wg/garages/{id}/peers` with `?version=` conditional GET is also available.

See [moto-club-websocket.md](moto-club-websocket.md) for additional WebSocket endpoints (log streaming, events - future).

### Garage Service

Manages wrenching environments.

**Create garage:**
```rust
struct CreateGarageRequest {
    name: Option<String>,       // Human-friendly name (auto-generated if omitted, IMMUTABLE)
    branch: Option<String>,     // Git branch (CLI determines from local repo if omitted)
    ttl_seconds: Option<u64>,   // Time-to-live in seconds (default: 14400 = 4h)
    image: Option<String>,      // Override dev container image
}

struct Garage {
    id: Uuid,
    name: String,               // Unique, human-friendly (e.g., "bold-mongoose"), IMMUTABLE
    owner: String,              // From Bearer token
    branch: String,
    status: GarageStatus,
    image: String,              // Dev container image used
    ttl_seconds: u64,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    namespace: String,          // K8s namespace: moto-garage-{id}
    pod_name: String,           // K8s pod name
    terminated_at: Option<DateTime<Utc>>,
    termination_reason: Option<String>,  // user_closed, ttl_expired, pod_lost, namespace_missing, error
}

enum GarageStatus {
    Pending,       // Pod scheduled, pulling images
    Initializing,  // Pod running, cloning repo, starting services
    Ready,         // Garage ready for use
    Failed,        // Startup failed (clone error, bad credentials, etc.)
    Terminated,    // Closed/cleaned up
}
```

**Create flow:**
```
1. Validate request
2. Generate ID (UUID), name (if not provided)
3. Insert into database (Pending, owner from config)
4. Create K8s namespace: moto-garage-{id}
5. Apply labels: moto.dev/type=garage, moto.dev/garage-id={id}, moto.dev/owner={owner}
6. Apply NetworkPolicy, ResourceQuota, LimitRange
7. Generate WireGuard keypair:
   - Create wireguard-config ConfigMap (address, peers)
   - Create wireguard-keys Secret (private_key, public_key)
   - Store public_key in database for client session routing
8. Issue garage SVID from keybox:
   - POST /auth/issue-garage-svid { garage_id, owner }
   - Create garage-svid Secret with returned SVID
9. If --with-postgres/--with-redis: create supporting service Deployments, Services, Secrets
10. Create workspace PVC
11. Deploy dev container pod (mounts all secrets/configmaps)
12. Wait for pod Ready (includes supporting services if requested)
13. Update database (Ready)
14. Return garage details
```

**Create failure handling:**
- If K8s fails after DB insert: mark garage as `terminated` with `termination_reason = "error"`, return `K8S_ERROR`
- If pod never becomes Ready (timeout): leave as `pending`, reconciler will eventually mark as terminated
- No rollback of DB record (kept for audit trail); orphaned namespaces cleaned by reconciler

**Extend TTL:**
```rust
struct ExtendTtlRequest {
    seconds: u64,   // Seconds to ADD to current expiry
}

// Example: garage expires in 30 min, extend by 7200 (2h)
// New expiry = now + 30min + 2h = 2h30m from now
```

**Constraints:**
- Cannot extend an expired garage (returns `GARAGE_EXPIRED` error)
- Cannot extend a terminated garage (returns `GARAGE_TERMINATED` error)
- Total TTL cannot exceed `MOTO_CLUB_MAX_TTL_SECONDS` (returns `INVALID_TTL` error)

**Close flow:**
```
1. Update database status to Terminated
2. Set terminated_at timestamp
3. Set termination_reason
4. Delete K8s namespace (cascades to all resources)
```

**Close failure handling:**
- DB update is done first (source of truth for "user requested close")
- If K8s namespace deletion fails: log error, return success anyway (user intent captured)
- Reconciler will retry namespace deletion on next cycle
- Idempotent: closing already-terminated garage succeeds (namespace may already be gone)

### Reconciliation

moto-club polls K8s to keep the database in sync. K8s is the source of truth.

**Poll interval:** 30 seconds (configurable)

**Reconciliation logic:**
```
For each garage namespace in K8s (label: moto.dev/type=garage):
  1. Get pod status
  2. If garage exists in DB:
     - Update status to match pod status
     - If pod missing/terminated and DB says Running/Ready:
       - Mark as Terminated
       - Set termination_reason = "pod_lost"
  3. If garage NOT in DB (orphan):
     - Log warning
     - Optionally: delete namespace (configurable)

For each garage in DB with status != Terminated:
  1. If no matching K8s namespace exists:
     - Mark as Terminated
     - Set termination_reason = "namespace_missing"
```

**Note:** TTL enforcement (closing expired garages) is NOT done here. That's handled by moto-cron, which calls the DELETE endpoint.

**Session expiry:**
- `GET /api/v1/wg/garages/{id}/peers` filters out expired sessions (checks `expires_at < now`)
- Expired sessions remain in database but are effectively inactive
- moto-cron periodically cleans up expired session records (sets `closed_at`, increments `peer_version`)
- When a garage is terminated, all its sessions are automatically closed

### WireGuard Coordination

moto-club coordinates WireGuard connections but never sees traffic.

**Responsibilities:**
- Device registration (client public keys)
- Session creation (authorize client → garage connections)
- Garage registration (garage public keys, called by garage pods)
- IP allocation (overlay network: fd00:moto::/48)
- DERP map distribution

**IP allocation algorithm:**

| Subnet | Range | Algorithm |
|--------|-------|-----------|
| Garages | `fd00:moto:1::/64` | Deterministic: first 8 bytes of SHA256(garage_id) as host part |
| Clients | `fd00:moto:2::/64` | Sequential: next available IP, stored in `wg_devices.assigned_ip` |

- Garage IPs are deterministic so same garage always gets same IP (even if re-registered)
- Client IPs are allocated sequentially and persisted; same device keeps same IP
- Collision/exhaustion: /64 provides 2^64 addresses (~18 quintillion). Exhaustion is practically impossible.
  If somehow exhausted or collision occurs, fail with `INTERNAL_ERROR` (operational issue, not user error)

**Garage Registration Validation:**

When a garage pod calls `POST /api/v1/wg/garages`, moto-club validates:

1. **Namespace isolation:** Pod must be running in `moto-garage-{id}` namespace
2. **K8s ServiceAccount token:** Validate the token via K8s TokenReview API, verify pod metadata matches claimed garage_id

This prevents rogue pods from registering as arbitrary garages.

**Peer Updates (v1 - REST Polling):**

For v1, garages poll for peer updates instead of WebSocket streaming:

```
GET /api/v1/wg/garages/{id}/peers
Authorization: Bearer <k8s-service-account-token>

Response 200:
{
  "peers": [
    { "public_key": "...", "allowed_ip": "fd00:moto:2::1/128" }
  ]
}
```

Garage daemon polls this endpoint every 5 seconds to get current peer list. WebSocket streaming will replace this in a future version.

**DERP server management:**

For v1, DERP servers are configured via config file (not runtime API):

```toml
# File: /etc/moto-club/derp.toml (or MOTO_CLUB_DERP_CONFIG env var)
[[regions]]
id = 1
name = "primary"

[[regions.nodes]]
host = "derp.example.com"
port = 443
stun_port = 3478
```

**Config loading:**
- Path: `MOTO_CLUB_DERP_CONFIG` env var, or `/etc/moto-club/derp.toml` default
- On startup: sync config to `derp_servers` table (insert/update/delete to match)
- Health check interval: 30 seconds
- Unhealthy threshold: 3 consecutive failures
- Unhealthy servers marked `healthy = false`, excluded from DERP map
- DERP map provided to clients/garages via session creation and garage registration APIs

**Future:** Admin API for runtime DERP management.

See [moto-wgtunnel.md](moto-wgtunnel.md) for tunnel architecture, connection flow, and client implementation details.

### Database Schema (PostgreSQL)

```sql
-- Garages (historical record, includes terminated)
CREATE TABLE garages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    owner TEXT NOT NULL,
    branch TEXT NOT NULL,
    status TEXT NOT NULL,           -- pending, initializing, ready, failed, terminated
    image TEXT NOT NULL,            -- dev container image used
    ttl_seconds INTEGER NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    namespace TEXT NOT NULL,
    pod_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    terminated_at TIMESTAMPTZ,          -- NULL if not terminated, set when status = terminated
    termination_reason TEXT              -- NULL if not terminated; required when terminated
                                         -- Values: user_closed, ttl_expired, pod_lost, namespace_missing, error
);

CREATE INDEX idx_garages_owner ON garages(owner);
CREATE INDEX idx_garages_status ON garages(status);
CREATE INDEX idx_garages_expires_at ON garages(expires_at) WHERE status != 'terminated';
-- Note: idx_garages_name not needed; UNIQUE constraint on 'name' creates implicit index

-- WireGuard devices (client devices)
-- WireGuard public key IS the device identity (Cloudflare WARP model)
CREATE TABLE wg_devices (
    public_key TEXT PRIMARY KEY,    -- WG public key is the identifier
    owner TEXT NOT NULL,
    device_name TEXT,               -- optional friendly name
    assigned_ip TEXT NOT NULL,      -- fd00:moto:2::xxx
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_wg_devices_owner ON wg_devices(owner);

-- WireGuard sessions (active tunnel sessions)
CREATE TABLE wg_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_pubkey TEXT NOT NULL REFERENCES wg_devices(public_key),
    garage_id UUID NOT NULL REFERENCES garages(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at TIMESTAMPTZ
);

CREATE INDEX idx_wg_sessions_device ON wg_sessions(device_pubkey);
CREATE INDEX idx_wg_sessions_garage ON wg_sessions(garage_id);
CREATE INDEX idx_wg_sessions_expires ON wg_sessions(expires_at) WHERE closed_at IS NULL;

-- Garage WireGuard registration (set by garage pod on startup)
CREATE TABLE wg_garages (
    garage_id UUID PRIMARY KEY REFERENCES garages(id) ON DELETE CASCADE,
    public_key TEXT NOT NULL UNIQUE,
    assigned_ip TEXT NOT NULL,          -- fd00:moto:1::xxx
    endpoints TEXT[] NOT NULL,          -- pod's reachable endpoints
    peer_version INTEGER NOT NULL DEFAULT 0,  -- incremented on session create/close
    registered_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- DERP servers (monitored by moto-club)
CREATE TABLE derp_servers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    region_id INTEGER NOT NULL,
    region_name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 443,
    stun_port INTEGER NOT NULL DEFAULT 3478,
    healthy BOOLEAN NOT NULL DEFAULT true,
    last_check_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```

### Error Handling

All API errors use a standard format:

```json
{
  "error": {
    "code": "GARAGE_NOT_FOUND",
    "message": "Garage 'bold-mongoose' not found",
    "details": {}
  }
}
```

**Error codes:**

| Code | HTTP Status | Description |
|------|-------------|-------------|
| `GARAGE_NOT_FOUND` | 404 | Garage doesn't exist |
| `GARAGE_NOT_OWNED` | 403 | Garage exists but owned by someone else |
| `GARAGE_ALREADY_EXISTS` | 409 | Garage name already taken |
| `GARAGE_TERMINATED` | 410 | Garage has been terminated |
| `GARAGE_EXPIRED` | 410 | Garage TTL has expired (cannot extend) |
| `GARAGE_NOT_REGISTERED` | 400 | Garage hasn't registered its WireGuard endpoint |
| `INVALID_TTL` | 400 | TTL out of valid range |
| `INVALID_STATUS` | 400 | Unknown status value in filter |
| `DEVICE_NOT_FOUND` | 404 | WireGuard device (public key) not registered |
| `DEVICE_NOT_OWNED` | 403 | Device (public key) belongs to different user |
| `SESSION_NOT_FOUND` | 404 | WireGuard session not found |
| `SESSION_NOT_OWNED` | 403 | Session belongs to different user |
| `INVALID_TOKEN` | 401 | K8s ServiceAccount token invalid or expired |
| `NAMESPACE_MISMATCH` | 403 | Pod not in expected garage namespace |
| `INTERNAL_ERROR` | 500 | Unexpected server error |
| `K8S_ERROR` | 502 | Kubernetes API error |
| `DATABASE_ERROR` | 503 | Database connection error |

### Health Check

```
GET /health

Response 200 (healthy):
{
  "status": "healthy",
  "checks": {
    "database": "ok",
    "k8s": "ok",
    "keybox": "ok"
  }
}

Response 200 (degraded):
{
  "status": "degraded",
  "checks": {
    "database": "ok",
    "k8s": "ok",
    "keybox": "error: connection refused"
  }
}
```

**Behavior:** Health check always returns 200 and reports status. Individual check failures don't cause hard failure - moto-club continues serving what it can.

### Logging

Structured JSON logging to stdout:

```json
{
  "timestamp": "2026-01-22T10:30:00Z",
  "level": "info",
  "message": "Garage created",
  "garage_id": "abc123",
  "garage_name": "bold-mongoose",
  "owner": "nick",
  "request_id": "req_xyz789"
}
```

**Log levels:**
- `error` - Unrecoverable failures
- `warn` - Recoverable issues, degraded state
- `info` - Request/response, lifecycle events
- `debug` - Internal state (off in prod)

### Configuration

```bash
# Required
MOTO_CLUB_DATABASE_URL="postgres://moto:password@localhost:5432/moto"
MOTO_CLUB_KEYBOX_URL="http://keybox:8080"

# K8s (auto-detected in-cluster, or specify for out-of-cluster)
KUBECONFIG="/path/to/kubeconfig"          # Optional, for local dev

# Optional
MOTO_CLUB_BIND_ADDR="0.0.0.0:8080"
MOTO_CLUB_MIN_TTL_SECONDS="60"            # 1 minute minimum
MOTO_CLUB_DEFAULT_TTL_SECONDS="14400"     # 4 hours
MOTO_CLUB_MAX_TTL_SECONDS="172800"        # 48 hours
MOTO_CLUB_DEV_CONTAINER_IMAGE="ghcr.io/nhalm/moto-dev:latest"
MOTO_CLUB_RECONCILE_INTERVAL_SECONDS="30"
MOTO_CLUB_DERP_CONFIG="/etc/moto-club/derp.toml"  # DERP server config file

# Logging
RUST_LOG="moto_club=info"
```

### K8s Integration

Uses `kube-rs` for K8s API interaction.

**Namespace per garage:**
```yaml
apiVersion: v1
kind: Namespace
metadata:
  name: moto-garage-{id}
  labels:
    moto.dev/type: garage
    moto.dev/garage-id: {id}
    moto.dev/garage-name: {name}
    moto.dev/owner: {owner}
```

**Labels used for filtering:**
- `moto.dev/type: garage` - All garage namespaces
- `moto.dev/garage-id: {id}` - Specific garage by ID
- `moto.dev/owner: {owner}` - All garages for an owner

## Deferred Items

### Bike Management (Future)

Bike service will handle:
- Deploy bikes (create K8s deployments)
- Track bike status
- Stop/restart bikes
- Scaling

This will be added in a future version. The crate structure is designed to accommodate `moto-club-bike/` when ready.

### Supporting Services

Per-garage supporting services (Postgres, Redis) are provisioned on demand. See [supporting-services.md](supporting-services.md).

**Integration:** When `--with-postgres` or `--with-redis` flags are passed to garage creation, moto-club must call `create_garage_postgres()` and `create_garage_redis()` to create the K8s Deployments, Services, and Secrets before creating the garage pod.

### Authentication (Future)

Identity system will replace config-based owner identity:
- API keys for CLI
- OIDC for web UI
- Service accounts for internal services

## Changelog

### v1.5 (2026-02-04)
- **Health check keybox integration:** `/health/ready` must check keybox `/health/ready`
  - Return degraded status if keybox unreachable
  - Add `keybox: "ok"` or `keybox: "unavailable"` to health response
- **WireGuard key storage (step 7):** Store garage public_key in `wg_garages` table during creation
  - Required for client session routing (clients need to know which garage to connect to)
  - Currently has TODO in code, must be implemented
- **Owner field in RegisteredDevice:** Add `owner` field to `RegisteredDevice` trait
  - Currently hardcoded as "unknown" in PostgresPeerStore
  - Required for proper device ownership tracking and ABAC

### v1.4
- Move WebSocket peer streaming from "Deferred" to implemented (WS /internal/wg/garages/{id}/peers exists)
- Update /api/v1/info features.websocket to return true (was incorrectly false)
- Move Supporting Services from "Future" to implemented
- Fix: Call create_garage_postgres() and create_garage_redis() in garage creation flow (services were not being created, only env vars injected)

### v1.3
- Updated garage creation flow with SVID provisioning:
  - Issue garage SVID from keybox (POST /auth/issue-garage-svid)
  - Create garage-svid Secret in garage namespace
- Added WireGuard keypair generation steps to create flow
- Added supporting services provisioning steps (--with-postgres/--with-redis)
- Added workspace PVC creation step
- Removed ServiceAccount step (garages use SVID push model, not K8s SA auth)

### v1.2
- Rename "Running" to "Initializing" garage status (clearer meaning)
- Add "Failed" garage status (for clone errors, bad credentials, startup failures)
- Replace SSH with ttyd + tmux for terminal access (tunnel is sole auth boundary)
- Remove SSH key management entirely:
  - Remove ssh_keys.rs from crate structure
  - Remove SSH key endpoints (POST/GET/DELETE /api/v1/users/ssh-keys)
  - Remove SSH Key Injection section
  - Remove user_ssh_keys table from schema
  - Remove INVALID_SSH_KEY error code
- Update garage creation flow (no SSH keys Secret step)
- Update connectivity model diagram (tunnel + ttyd)

### v1.1
- Added `ON DELETE CASCADE` to `wg_sessions.garage_id` FK (prevents orphaned records)
- Removed unreachable `Attached` garage status (no mechanism to detect WireGuard connection)
- Specified SSH key injection mechanism (K8s Secret mounted to garage pod)
- Updated garage creation flow to include SSH keys secret creation

### v1.0
- Removed unused `SESSION_EXPIRED` error code
- Added `GET /api/v1/wg/garages/{garage_id}` endpoint for garage WG registration retrieval
- Added conditional GET support for peer polling (`?version=` query param, 304 response)
- Added `GET /api/v1/wg/derp-map` endpoint for DERP server discovery
- Clarified TTL capping detection via `ttl_capped` field in session creation response
- Added moto-cron.md to References section

### v0.9
- Added `INVALID_SSH_KEY` error code for malformed SSH public keys
- Added query parameters to List Sessions (`?garage_id=`, `?all=`)
- Clarified DELETE session is soft delete (sets `closed_at`, idempotent)
- Added `MOTO_CLUB_DERP_CONFIG` to configuration section
- Added `GARAGE_EXPIRED` error to session creation (for expired but not-yet-terminated garages)
- Clarified IP exhaustion behavior (practically impossible with /64 space)
- Added `updated_at` to Create Garage response for consistency
- Clarified `termination_reason` nullability in schema comments
- Removed redundant `idx_garages_name` index (UNIQUE already creates one)

### v0.8
- Fixed SSH key registration: 200 for idempotent re-registration (not 409)
- Specified peer_version increment behavior (on session create/close)
- Added session expiry cleanup specification (moto-cron handles cleanup)
- Added `INVALID_STATUS` error for invalid status filter values
- Added TTL validation: min 60s, clarified total TTL calculation for extend
- Specified garage name generation algorithm (adjective-animal, collision handling)
- Specified IP allocation algorithm (SHA256 hash for garages, sequential for clients)
- Added create/close failure handling and rollback behavior
- Specified DERP config file location and health check parameters
- Clarified garage WG re-registration allows pubkey update (security via namespace validation)
- Added new error codes: `GARAGE_NOT_REGISTERED`, `INVALID_STATUS`
- Added config: `MOTO_CLUB_MIN_TTL_SECONDS`, `MOTO_CLUB_DERP_CONFIG`

### v0.7
- Added full API specifications for all garage management endpoints (POST, GET, DELETE, extend)
- Added `image` column to garages table
- Added `peer_version` column to `wg_garages` table for change detection
- Aligned Garage struct with database schema (added `image`, `updated_at`, `terminated_at`, `termination_reason`)
- Added query parameters for `GET /api/v1/garages` (`?status=`, `?all=`)

### v0.6
- Added `wg_garages` table for garage WireGuard registration
- Added garage registration error codes: `INVALID_TOKEN`, `NAMESPACE_MISMATCH`
- Clarified session TTL behavior: capped to garage remaining TTL
- Added DERP server configuration (via config file for v1)
- Added `GET /api/v1/info` response format

### v0.5
- Simplified device identity: WireGuard public key IS the device identifier (Cloudflare WARP model)
- Removed `device_id` concept - no separate device UUID needed
- Updated schema: `wg_devices.public_key` is now primary key
- Updated APIs to use `device_pubkey` instead of `device_id`
- Removed `DEVICE_ALREADY_EXISTS` error (re-registration is now idempotent 200)

### v0.4
- Added detailed WireGuard API specifications (request/response bodies, behaviors, errors)
- Added new error codes: `DEVICE_NOT_OWNED`, `DEVICE_ALREADY_EXISTS`, `SESSION_NOT_OWNED`
- Added SSH key management endpoints: `GET /api/v1/users/ssh-keys`, `DELETE /api/v1/users/ssh-keys/{id}`

### v0.3
- Initial specification

## References

- [garage-lifecycle.md](garage-lifecycle.md) - Garage state machine
- [moto-wgtunnel.md](moto-wgtunnel.md) - WireGuard tunnel system
- [moto-club-websocket.md](moto-club-websocket.md) - WebSocket streaming
- [moto-cron.md](moto-cron.md) - Scheduled tasks (TTL enforcement, session cleanup)
- [moto-cli.md](moto-cli.md) - CLI commands
- [keybox.md](keybox.md) - Secrets management
