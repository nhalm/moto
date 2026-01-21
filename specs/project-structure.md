# Project Structure Specification

**Version:** 1.1
**Last Updated:** 2026-01-21

---

## 1. Overview

### Purpose

This specification defines the directory layout, crate organization, and structural conventions for the moto monorepo. It answers WHERE things go and HOW they relate - not HOW to implement them.

### Goals

- Clear boundaries between crates, infrastructure, and documentation
- Consistent naming using the `moto-` prefix and motorcycle metaphor
- Workspace efficiency with shared dependencies
- Spec-driven development where specs steer, agents implement

### Non-Goals

- Web frontend (no JavaScript/TypeScript in v1)
- Mobile apps

---

## 2. System Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              User Workstation                                │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│    ┌──────────────┐                                                          │
│    │   moto-cli   │ ◄── Handlebars (steering)                               │
│    │    (Bars)    │                                                          │
│    └──────┬───────┘                                                          │
│           │                                                                  │
│           │ REST/WebSocket                                                   │
│           ▼                                                                  │
│    ┌──────────────────────────────────────────────────────────────────────┐ │
│    │                         moto-club (Server)                           │ │
│    │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐ │ │
│    │  │ club-api    │  │ club-ws     │  │ club-garage │  │ club-bike   │ │ │
│    │  │ (REST)      │  │ (WebSocket) │  │ (Garage Svc)│  │ (Bike Svc)  │ │ │
│    │  └─────────────┘  └─────────────┘  └─────────────┘  └─────────────┘ │ │
│    │                           │                                          │ │
│    │                    ┌──────┴──────┐                                   │ │
│    │                    │  club-k8s   │                                   │ │
│    │                    │  club-db    │                                   │ │
│    │                    └──────┬──────┘                                   │ │
│    └───────────────────────────┼──────────────────────────────────────────┘ │
│                                │                                             │
│                                ▼                                             │
│    ┌──────────────────────────────────────────────────────────────────────┐ │
│    │                         k3s Cluster (Frame)                          │ │
│    │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐      │ │
│    │  │ Garage Namespace│  │ Garage Namespace│  │  Bike Namespace │      │ │
│    │  │ (dev-alice-123) │  │ (dev-bob-456)   │  │  (prod-v1.2.3)  │      │ │
│    │  └─────────────────┘  └─────────────────┘  └─────────────────┘      │ │
│    │                                                                      │ │
│    │  ┌─────────────────────────────────────────────────────────────┐    │ │
│    │  │  PostgreSQL    Redis    moto-keybox    moto-ai-proxy        │    │ │
│    │  └─────────────────────────────────────────────────────────────┘    │ │
│    └──────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Crate Dependency Graph

```
                                 moto-cli (binary)
                                       │
                    ┌──────────────────┼──────────────────┐
                    │                  │                  │
                    ▼                  ▼                  ▼
             moto-club-types    moto-garage       moto-k8s
                    │                  │                  │
                    └──────────┬───────┴──────────────────┘
                               │
                               ▼
                         moto-common


                                moto-club (binary)
                                       │
              ┌────────────┬───────────┼───────────┬────────────┐
              │            │           │           │            │
              ▼            ▼           ▼           ▼            ▼
        moto-club-api  club-ws  club-garage  club-bike  club-db
              │            │           │           │            │
              └────────────┴───────────┼───────────┴────────────┘
                                       │
                    ┌──────────────────┼──────────────────┐
                    ▼                  ▼                  ▼
             moto-club-k8s     moto-club-types     moto-common
```

---

## 4. Directory Structure

```
moto/
├── .cargo/
│   └── config.toml                   # Cargo build settings
│
├── crates/                           # All Rust code
│   │
│   │── moto-common/                  # Shared utilities (errors, config, Secret<T>)
│   │── moto-cli/                     # Binary: CLI (the Bars)
│   │── moto-garage/                  # Library: garage client logic
│   │── moto-k8s/                     # Library: Kubernetes operations
│   │
│   │── moto-club/                    # Binary: central server
│   │── moto-club-api/                # Library: REST handlers
│   │── moto-club-ws/                 # Library: WebSocket handlers
│   │── moto-club-garage/             # Library: garage service logic
│   │── moto-club-bike/               # Library: bike service logic
│   │── moto-club-k8s/                # Library: K8s operations for club
│   │── moto-club-db/                 # Library: database layer
│   │── moto-club-types/              # Library: shared types (CLI + Club)
│   │
│   │── moto-keybox/                  # Secrets manager (future)
│   │── moto-ai-proxy/                # AI provider proxy (future)
│   │── moto-tank/                    # Vault/storage (future)
│   │── moto-transmission/            # Proxy layer (future)
│   │── moto-exhaust/                 # Logging/audit (future)
│   │── moto-throttle/                # Rate limiting (future)
│   │── moto-brakes/                  # Circuit breakers (future)
│   │── moto-mirrors/                 # Observability (future)
│   │── moto-switches/                # Feature flags (future)
│   │
│   └── engines/                      # Business services (future)
│       ├── moto-tokenization/
│       ├── moto-payments/
│       └── moto-lending/
│
├── docker/
│   ├── garage.Dockerfile             # Dev container image
│   ├── club.Dockerfile               # Club server image
│   └── bike.Dockerfile               # Bike runtime image
│
├── infra/
│   ├── k8s/
│   │   ├── cluster/                  # k3s cluster setup
│   │   ├── garage/                   # Garage namespace templates
│   │   ├── bike/                     # Bike namespace templates
│   │   └── services/                 # PostgreSQL, Redis, etc.
│   │
│   ├── nix/
│   │   ├── modules/                  # NixOS modules
│   │   ├── pkgs/                     # Nix packages
│   │   └── shells/                   # Dev shells
│   │
│   └── scripts/                      # Setup scripts
│
├── specs/                            # Specifications
│   ├── README.md
│   └── *.md
│
├── .gitignore
├── AGENTS.md                         # Agent guidelines
├── CLAUDE.md                         # Points to AGENTS.md
├── Cargo.toml                        # Workspace manifest
├── Makefile
├── flake.nix
├── rust-toolchain.toml
└── tracks.md                         # Implementation log
```

---

## 5. Crate Purposes

### Core Crates (Implement First)

| Crate | Type | Purpose |
|-------|------|---------|
| `moto-common` | lib | Shared utilities: error types, config loading, `Secret<T>` wrapper |
| `moto-club-types` | lib | Types shared between CLI and server: `GarageId`, `GarageInfo`, `GarageState`, API request/response types |
| `moto-k8s` | lib | Low-level K8s operations: namespace CRUD, pod CRUD, label helpers |
| `moto-garage` | lib | Garage client: abstracts local (direct K8s) vs remote (via club) mode |
| `moto-cli` | bin | CLI commands: `moto garage {list,open,close}`, `moto bike {...}` |

### Club Crates (Implement Later)

| Crate | Type | Purpose |
|-------|------|---------|
| `moto-club` | bin | Server main: wires together API, WS, services |
| `moto-club-api` | lib | REST handlers for garage/bike operations |
| `moto-club-ws` | lib | WebSocket for terminal attach, real-time updates |
| `moto-club-garage` | lib | Garage service: lifecycle state machine, TTL |
| `moto-club-bike` | lib | Bike service: deployment, scaling |
| `moto-club-k8s` | lib | K8s operations specific to club (wraps moto-k8s) |
| `moto-club-db` | lib | Database: migrations, repositories |

### Infrastructure Crates (Future)

| Crate | Metaphor | Purpose |
|-------|----------|---------|
| `moto-keybox` | Keybox | SPIFFE-based secrets manager |
| `moto-ai-proxy` | AI Proxy | Routes to AI providers, injects secrets |
| `moto-tank` | Tank | Encrypted vault storage |
| `moto-transmission` | Transmission | Proxy layer |
| `moto-exhaust` | Exhaust | Audit logging |
| `moto-throttle` | Throttle | Rate limiting |
| `moto-brakes` | Brakes | Circuit breakers |
| `moto-mirrors` | Mirrors | Observability |
| `moto-switches` | Switches | Feature flags |

---

## 6. Key Types

### moto-club-types

**GarageId** - UUID v7 newtype with `.short()` for display (first 8 chars)

**GarageState** - Enum: `Pending`, `Running`, `Ready`, `Terminating`, `Terminated`

**GarageInfo** - Struct:
- `id: GarageId`
- `name: String` (human-friendly)
- `namespace: String` (K8s namespace name)
- `state: GarageState`
- `created_at: DateTime<Utc>`
- `expires_at: Option<DateTime<Utc>>`
- `owner: Option<String>`

**BikeId**, **BikeState**, **BikeInfo** (future) - Similar pattern for bikes

**API Types** (future) - `CreateGarageRequest`, `CreateGarageResponse`, `ListGaragesRequest`, etc.

### moto-k8s

**K8sClient** - Wraps `kube::Client`, provides namespace/pod operations

**NamespaceOps** - Trait: `create_namespace()`, `delete_namespace()`, `list_namespaces()`, `get_namespace()`

**Labels** - Constants: `moto.dev/type`, `moto.dev/id`, `moto.dev/name`, `moto.dev/owner`

### moto-garage

**GarageMode** - Enum: `Local` (direct K8s) or `Remote { endpoint }` (via club)

**GarageClient** - Methods: `list()`, `open(name)`, `close(id)`

---

## 7. Workspace Dependencies

Key external crates to use:

| Purpose | Crate | Version |
|---------|-------|---------|
| Async runtime | tokio | 1.x |
| Web framework | axum | 0.8 |
| Kubernetes | kube, k8s-openapi | latest |
| Database | sqlx | 0.8 |
| CLI | clap | 4.x |
| Serialization | serde, serde_json | 1.x |
| Errors | thiserror | 2.x |
| Logging | tracing | 0.1 |
| Time | chrono | 0.4 |
| IDs | uuid (v7) | 1.x |
| Testing | proptest | 1.x |

Use workspace dependencies in root `Cargo.toml` so all crates share versions.

---

## 8. Naming Conventions

### Crates
- `moto-{component}` for standalone crates
- `moto-club-{sub}` for club server components
- `moto-{metaphor}` for infrastructure aligned with bike parts

### Kubernetes Labels
All labels use `moto.dev/` prefix:
- `moto.dev/type` - "garage" or "bike"
- `moto.dev/id` - UUID
- `moto.dev/name` - human-friendly name
- `moto.dev/owner` - owner identifier

### Namespace Names
- Garages: `moto-garage-{short_id}` (8 char UUID prefix)
- Bikes: `moto-bike-{name}-{version}`

---

## 9. Key Decisions

### Local vs Remote Mode
The CLI can work in two modes:
- **Local**: Direct K8s access via kubeconfig (for solo dev)
- **Remote**: Through moto-club server (for team/managed)

`moto-garage` abstracts this - same interface, different backend.

### Namespace-First
Garages are Kubernetes namespaces with labels. In v1, we just manage namespaces. Pods, volumes, services come later.

### Types Shared via moto-club-types
Both CLI and server need the same types (GarageInfo, etc). Put them in `moto-club-types` which has no server dependencies - just serde, chrono, uuid.

---

## 10. Files to Create

### Root Config Files

**Cargo.toml** - Workspace manifest with:
- `[workspace]` with members list
- `[workspace.package]` with shared metadata
- `[workspace.dependencies]` with pinned versions
- `[workspace.lints]` for clippy/rustc settings

**rust-toolchain.toml** - Pin to stable channel (1.84+)

**Makefile** - Targets: `build`, `test`, `check`, `fmt`, `lint`, `clean`, `run`

**.gitignore** - Rust, Nix, editor, secrets patterns

---

## Appendix: Motorcycle Metaphor

| Bike Part | System Concept | Crate |
|-----------|----------------|-------|
| Club | Central orchestration | moto-club |
| Garage | Dev environment | moto-garage |
| Bike | Production deployment | moto-club-bike |
| Bars | CLI/control | moto-cli |
| Frame | K8s infrastructure | moto-k8s |
| Tank | Encrypted storage | moto-tank |
| Transmission | Proxy | moto-transmission |
| Exhaust | Logging/audit | moto-exhaust |
| Throttle | Rate limiting | moto-throttle |
| Brakes | Circuit breakers | moto-brakes |
| Mirrors | Observability | moto-mirrors |
| Switches | Feature flags | moto-switches |
| Keybox | Secrets | moto-keybox |
| Engine | Business services | moto-tokenization, etc. |

---

## Changelog

### v1.1 (2026-01-21)

Renamed `moto-k3s` → `moto-k8s`. The crate is K8s-agnostic; k3s is an infrastructure choice, not a code dependency.

### v1.0 (2026-01-20)

Initial spec.
