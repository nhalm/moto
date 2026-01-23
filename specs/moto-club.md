# Moto Club

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Last Updated | 2026-01-23 |

## Overview

The motorcycle club - central orchestration service for the moto platform. Where the riders gather and garages are managed. Handles garage lifecycles, WireGuard coordination for remote access, and K8s coordination.

**Scope for v1:**
- Garage management (create, track, close)
- WireGuard coordination (peer registration, IP allocation)
- K8s namespace/pod management
- Poll-based reconciliation (K8s → database)

**Deferred:**
- Bike management (deploy, track, stop) - future version
- Supporting services per garage (Postgres, Redis) - future version
- TTL enforcement loop (handled by moto-cron) - manual `garage close` for now
- Authentication/identity system
- WebSocket streaming (REST polling for v1)
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

1. **WireGuard** - Terminal/SSH access to garages
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
│   (local)    │   tunnel + SSH     │   (remote)   │
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
│       ├── ssh_keys.rs       # User SSH key management
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

```
POST   /api/v1/garages              Create a garage
GET    /api/v1/garages              List garages (filtered by owner)
GET    /api/v1/garages/{name}       Get garage details
DELETE /api/v1/garages/{name}       Close/delete garage
POST   /api/v1/garages/{name}/extend  Extend TTL (add seconds)
```

#### WireGuard Coordination

```
POST   /api/v1/wg/devices           Register client device
GET    /api/v1/wg/devices/{id}      Get device info
POST   /api/v1/wg/sessions          Create tunnel session
GET    /api/v1/wg/sessions          List active sessions
DELETE /api/v1/wg/sessions/{id}     Close session
POST   /api/v1/wg/garages           Register garage (called by garage pod)
GET    /api/v1/wg/garages/{id}/peers  Get peer list (garage polls this)
POST   /api/v1/users/ssh-keys       Register user SSH key
```

#### Health/Info

```
GET    /health                      Health check (degrades gracefully)
GET    /api/v1/info                 Server info, version
```

#### WebSocket Endpoints (Deferred)

WebSocket streaming is deferred to a future version. See [moto-club-websocket.md](moto-club-websocket.md).

For v1, peer updates use REST polling (see `GET /api/v1/wg/garages/{id}/peers`).

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
    ttl_seconds: u64,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    namespace: String,          // K8s namespace: moto-garage-{id}
    pod_name: String,           // K8s pod name
}

enum GarageStatus {
    Pending,      // Pod scheduled, pulling images
    Running,      // Container started, initializing
    Ready,        // Garage ready for use
    Attached,     // User connected via WireGuard tunnel
    Terminated,   // Closed/cleaned up
}
```

**Create flow:**
```
1. Validate request
2. Generate ID (UUID), name (if not provided)
3. Insert into database (Pending, owner from config)
4. Create K8s namespace: moto-garage-{id}
5. Apply labels: moto.dev/type=garage, moto.dev/garage-id={id}, moto.dev/owner={owner}
6. Apply NetworkPolicy, ResourceQuota
7. Create ServiceAccount (for keybox auth)
8. Deploy dev container pod
9. Wait for pod Ready
10. Update database (Ready)
11. Return garage details
```

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

### WireGuard Coordination

moto-club coordinates WireGuard connections but never sees traffic.

**Responsibilities:**
- Device registration (client public keys)
- Session creation (authorize client → garage connections)
- Garage registration (garage public keys, called by garage pods)
- IP allocation (overlay network: fd00:moto::/48)
- DERP map distribution
- User SSH key storage (injected into garages)

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

**DERP monitoring:**
- DERP servers run as separate service (moto-derp)
- moto-club monitors DERP health
- Provides DERP map to clients and garages

See [moto-wgtunnel.md](moto-wgtunnel.md) for detailed API contracts.

### Database Schema (PostgreSQL)

```sql
-- Garages (historical record, includes terminated)
CREATE TABLE garages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE,
    owner TEXT NOT NULL,
    branch TEXT NOT NULL,
    status TEXT NOT NULL,           -- pending, running, ready, attached, terminated
    ttl_seconds INTEGER NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    namespace TEXT NOT NULL,
    pod_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    terminated_at TIMESTAMPTZ,
    termination_reason TEXT         -- user_closed, ttl_expired, pod_lost, namespace_missing, error
);

CREATE INDEX idx_garages_owner ON garages(owner);
CREATE INDEX idx_garages_status ON garages(status);
CREATE INDEX idx_garages_expires_at ON garages(expires_at) WHERE status != 'terminated';
CREATE INDEX idx_garages_name ON garages(name);

-- WireGuard devices (client devices)
CREATE TABLE wg_devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner TEXT NOT NULL,
    device_name TEXT,               -- optional friendly name
    public_key TEXT NOT NULL UNIQUE,
    assigned_ip TEXT NOT NULL,      -- fd00:moto:2::xxx
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_wg_devices_owner ON wg_devices(owner);

-- WireGuard sessions (active tunnel sessions)
CREATE TABLE wg_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id UUID NOT NULL REFERENCES wg_devices(id),
    garage_id UUID NOT NULL REFERENCES garages(id),
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    closed_at TIMESTAMPTZ
);

CREATE INDEX idx_wg_sessions_device ON wg_sessions(device_id);
CREATE INDEX idx_wg_sessions_garage ON wg_sessions(garage_id);
CREATE INDEX idx_wg_sessions_expires ON wg_sessions(expires_at) WHERE closed_at IS NULL;

-- User SSH keys
CREATE TABLE user_ssh_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner TEXT NOT NULL,
    public_key TEXT NOT NULL,
    fingerprint TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(owner, fingerprint)
);

CREATE INDEX idx_user_ssh_keys_owner ON user_ssh_keys(owner);

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
| `INVALID_TTL` | 400 | TTL out of valid range |
| `DEVICE_NOT_FOUND` | 404 | WireGuard device not found |
| `SESSION_NOT_FOUND` | 404 | WireGuard session not found |
| `SESSION_EXPIRED` | 410 | Session has expired |
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
MOTO_CLUB_DEFAULT_TTL_SECONDS="14400"     # 4 hours
MOTO_CLUB_MAX_TTL_SECONDS="172800"        # 48 hours
MOTO_CLUB_DEV_CONTAINER_IMAGE="ghcr.io/nhalm/moto-dev:latest"
MOTO_CLUB_RECONCILE_INTERVAL_SECONDS="30"

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

### Supporting Services (Future)

Per-garage supporting services (Postgres, Redis) will be added later. For v1, garages connect to shared/external services.

### Authentication (Future)

Identity system will replace config-based owner identity:
- API keys for CLI
- OIDC for web UI
- Service accounts for internal services

## References

- [garage-lifecycle.md](garage-lifecycle.md) - Garage state machine
- [moto-wgtunnel.md](moto-wgtunnel.md) - WireGuard tunnel system
- [moto-club-websocket.md](moto-club-websocket.md) - WebSocket streaming
- [moto-cli.md](moto-cli.md) - CLI commands
- [keybox.md](keybox.md) - Secrets management
