# Moto Club

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

The motorcycle club - central orchestration service for the moto platform. Where the riders gather, garages are managed, and bikes get dispatched. Handles garage and bike lifecycles, WebSocket relay for remote execution, TTL enforcement, and K8s coordination.

## Jobs to Be Done

- [x] Define server responsibilities
- [x] Define architecture
- [x] Define API endpoints
- [x] Define WebSocket protocol
- [x] Define garage management
- [x] Define bike management
- [ ] Define authentication/authorization
- [ ] Define database schema
- [ ] Define configuration

## Specification

### Responsibilities

| Domain | Responsibility |
|--------|----------------|
| **Garages** | Create, track, attach, cleanup wrenching environments |
| **Bikes** | Deploy, track, stop ripping instances |
| **WebSocket** | Relay stdin/stdout between CLI and pods |
| **TTL** | Enforce time-to-live, auto-cleanup |
| **K8s** | Create namespaces, deploy pods, manage services |
| **Registry** | Track all garages and bikes, their state |

### Architecture

```
                                    ┌─────────────────────────────────────┐
                                    │            moto-club              │
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
                                    │  │  - BikeService              │   │
                                    │  │  - TTLEnforcer              │   │
                                    │  │  - K8sClient                │   │
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

### Crate Structure

Broken into isolated crates for separation of concerns:

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
│       ├── bikes.rs          # Bike endpoints
│       └── health.rs         # Health/info endpoints
│
├── moto-club-ws/             # Library: WebSocket handlers
│   └── src/
│       ├── lib.rs
│       ├── attach.rs         # Terminal attachment
│       ├── logs.rs           # Log streaming
│       └── protocol.rs       # Frame types, encoding
│
├── moto-club-garage/         # Library: Garage service logic
│   └── src/
│       ├── lib.rs
│       ├── service.rs        # GarageService
│       ├── lifecycle.rs      # State transitions
│       └── ttl.rs            # TTL enforcement
│
├── moto-club-bike/           # Library: Bike service logic
│   └── src/
│       ├── lib.rs
│       ├── service.rs        # BikeService
│       ├── deploy.rs         # Deployment logic
│       └── scaling.rs        # Replica management
│
├── moto-club-k8s/            # Library: Kubernetes interactions
│   └── src/
│       ├── lib.rs
│       ├── namespace.rs      # Namespace management
│       ├── pods.rs           # Pod lifecycle
│       ├── exec.rs           # Pod exec/attach
│       └── resources.rs      # Quotas, policies
│
├── moto-club-db/             # Library: Database layer
│   └── src/
│       ├── lib.rs
│       ├── models.rs         # Data models
│       ├── garage_repo.rs    # Garage repository
│       └── bike_repo.rs      # Bike repository
│
└── moto-club-types/          # Library: Shared types (used by CLI too)
    └── src/
        ├── lib.rs
        ├── garage.rs         # Garage types
        ├── bike.rs           # Bike types
        └── api.rs            # Request/response types
```

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
    ┌────────────┼────────────┐
    │            │            │
    ▼            ▼            ▼
moto-club-   moto-club-   moto-club-db
  garage       bike            │
    │            │             │
    └─────┬──────┘             │
          │                    │
          ▼                    │
    moto-club-k8s              │
          │                    │
          └────────┬───────────┘
                   │
                   ▼
            moto-club-types
```

**Isolation benefits:**
- Each crate can be tested independently
- Clear boundaries between concerns
- Smaller compilation units
- Easier to understand and modify
- Can swap implementations (e.g., different K8s backends)

### REST API Endpoints

**Garage Management:**
```
POST   /api/v1/garages              Create a garage
GET    /api/v1/garages              List all garages
GET    /api/v1/garages/{id}         Get garage details
DELETE /api/v1/garages/{id}         Close/delete garage
POST   /api/v1/garages/{id}/extend  Extend TTL
POST   /api/v1/garages/{id}/sync    Trigger sync operation
```

**Bike Management:**
```
POST   /api/v1/bikes                Deploy a bike
GET    /api/v1/bikes                List all bikes
GET    /api/v1/bikes/{id}           Get bike details
DELETE /api/v1/bikes/{id}           Stop bike
GET    /api/v1/bikes/{id}/logs      Get bike logs
POST   /api/v1/bikes/{id}/restart   Restart bike
```

**Health/Info:**
```
GET    /health                      Health check
GET    /api/v1/info                 Server info, version
```

### WebSocket Endpoints

**Garage attachment:**
```
/ws/v1/garages/{id}/attach
```

Bidirectional stream for terminal I/O.

**Protocol:**

```
Frame types:
- 0x01: stdin (binary)      CLI → Server → Pod
- 0x02: stdout (binary)     Pod → Server → CLI
- 0x03: stderr (binary)     Pod → Server → CLI
- 0x10: resize (JSON)       {"cols": 80, "rows": 24}
- 0x11: heartbeat           Both directions, every 30s
- 0x12: detach              CLI → Server (graceful disconnect)
- 0xFF: error (JSON)        {"code": "...", "message": "..."}
```

**Bike logs:**
```
/ws/v1/bikes/{id}/logs?follow=true&tail=100
```

Streams log output, similar to `kubectl logs -f`.

### Garage Service

Manages wrenching environments.

**Create garage:**
```rust
struct CreateGarageRequest {
    name: Option<String>,       // Human-friendly name
    branch: Option<String>,     // Git branch (default: current)
    ttl: Option<Duration>,      // Time-to-live (default: 4h)
    image: Option<String>,      // Override dev container image
}

struct Garage {
    id: Uuid,
    name: String,
    branch: String,
    status: GarageStatus,
    ttl: Duration,
    expires_at: DateTime<Utc>,
    created_at: DateTime<Utc>,
    namespace: String,          // K8s namespace
    pod_name: String,           // K8s pod name
}

enum GarageStatus {
    Pending,
    Running,
    Ready,
    Attached,
    Terminated,
}
```

**Create flow:**
```
1. Validate request
2. Generate ID, name (if not provided)
3. Insert into database (Pending)
4. Create K8s namespace: moto-garage-{id}
5. Apply NetworkPolicy, ResourceQuota
6. Create ServiceAccount (for keybox auth)
7. Deploy dev container pod
8. Deploy supporting services (Postgres, Redis)
9. Wait for pod Ready
10. Update database (Ready)
11. Return garage details
```

### Bike Service

Manages ripping instances.

**Deploy bike:**
```rust
struct DeployBikeRequest {
    engine: String,             // Which engine (tokenization, payments, etc.)
    version: String,            // Version/tag to deploy
    replicas: Option<u32>,      // Number of replicas (default: 1)
    env: Option<HashMap<String, String>>,  // Environment overrides
}

struct Bike {
    id: Uuid,
    engine: String,
    version: String,
    status: BikeStatus,
    replicas: u32,
    created_at: DateTime<Utc>,
    namespace: String,
    deployment_name: String,
}

enum BikeStatus {
    Deploying,
    Running,
    Degraded,      // Some replicas unhealthy
    Stopped,
}
```

### TTL Enforcer

Background task that enforces garage TTLs.

```
Loop every 1 minute:
  1. Query garages where expires_at < now + 15min
  2. For each:
     - If expires_at < now: close garage
     - If expires_at < now + 5min: final warning
     - If expires_at < now + 15min: first warning
  3. Send warnings via WebSocket (if attached)
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
```

**Pod exec for attachment:**
```rust
// Attach to pod for stdin/stdout relay
let attached = pods
    .exec(pod_name, vec!["bash"])
    .stdin()
    .stdout()
    .stderr()
    .spawn()?;
```

### Database Schema (PostgreSQL)

```sql
-- Garages
CREATE TABLE garages (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    branch TEXT NOT NULL,
    status TEXT NOT NULL,           -- pending, running, ready, attached, terminated
    ttl_seconds INTEGER NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    namespace TEXT NOT NULL,
    pod_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    terminated_at TIMESTAMPTZ
);

CREATE INDEX idx_garages_status ON garages(status);
CREATE INDEX idx_garages_expires_at ON garages(expires_at) WHERE status != 'terminated';

-- Bikes
CREATE TABLE bikes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    engine TEXT NOT NULL,
    version TEXT NOT NULL,
    status TEXT NOT NULL,           -- deploying, running, degraded, stopped
    replicas INTEGER NOT NULL,
    namespace TEXT NOT NULL,
    deployment_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    stopped_at TIMESTAMPTZ
);

CREATE INDEX idx_bikes_status ON bikes(status);
CREATE INDEX idx_bikes_engine ON bikes(engine);
```

### Configuration

```bash
# Required
MOTO_SERVER_DATABASE_URL="postgres://moto:password@localhost:5432/moto"
MOTO_SERVER_KEYBOX_URL="http://keybox:8080"

# K8s (auto-detected in-cluster, or specify for out-of-cluster)
KUBECONFIG="/path/to/kubeconfig"          # Optional, for local dev

# Optional
MOTO_SERVER_BIND_ADDR="0.0.0.0:8080"
MOTO_SERVER_DEFAULT_TTL_SECONDS="14400"   # 4 hours
MOTO_SERVER_MAX_TTL_SECONDS="172800"      # 48 hours
MOTO_SERVER_DEV_CONTAINER_IMAGE="ghcr.io/nhalm/moto-dev:latest"
```

### Authentication (TODO)

Options to consider:
- API keys for CLI
- OIDC for web UI (future)
- Service accounts for internal services

### Health Check

```
GET /health

Response:
{
  "status": "healthy",
  "checks": {
    "database": "ok",
    "k8s": "ok",
    "keybox": "ok"
  }
}
```
