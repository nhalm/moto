# Project Structure Specification

**Version:** 1.7
**Status:** Ripping
**Last Updated:** 2026-03-05

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
│   │── moto-club-garage/             # Library: garage service logic
│   │── moto-club-k8s/                # Library: K8s operations for club
│   │── moto-club-db/                 # Library: database layer
│   │── moto-club-types/              # Library: shared types (CLI + Club)
│   │── moto-club-wg/                 # Library: WireGuard coordination
│   │── moto-club-reconcile/          # Library: K8s → DB reconciliation
│   │── moto-club-ws/                 # Library: WebSocket handlers for peer streaming
│   │
│   │── moto-wgtunnel-types/          # Library: WireGuard types
│   │── moto-wgtunnel-conn/           # Library: MagicConn, STUN
│   │── moto-wgtunnel-derp/           # Library: DERP protocol
│   │── moto-wgtunnel-engine/         # Library: Tunnel management
│   │── moto-cli-wgtunnel/            # Library: CLI tunnel integration
│   │── moto-garage-wgtunnel/         # Library: Garage daemon
│   │
│   │── moto-keybox/                  # Library: Secrets manager core
│   │── moto-keybox-server/           # Binary: Keybox HTTP server
│   │── moto-keybox-client/           # Library: Keybox client
│   │── moto-keybox-cli/              # Binary: Keybox admin CLI
│   │── moto-keybox-db/               # Library: Keybox database layer
│   │
│   │── moto-test-utils/              # Library: Test utilities for integration tests
│   │
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
├── infra/
│   ├── k8s/
│   │   └── moto-system/              # K8s manifests for control plane
│   │
│   ├── modules/                      # Nix modules (base, dev-tools, terminal, wireguard)
│   │
│   └── pkgs/                         # Nix package definitions (garage, bike, club, keybox)
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
├── specd_work_list.md                # Remaining work items
├── specd_history.md                  # Done log
└── specd_review.md                   # Items needing human review
```

---

## 5. Crate Purposes

### Core Crates (Implement First)

| Crate | Type | Purpose |
|-------|------|---------|
| `moto-common` | lib | Shared utilities: error types, config loading, `Secret<T>` wrapper |
| `moto-club-types` | lib | Types shared between CLI and server: `GarageId` |
| `moto-k8s` | lib | Low-level K8s operations: namespace CRUD, pod CRUD, label helpers |
| `moto-garage` | lib | **DEPRECATED** - Remove this crate. CLI talks directly to moto-club API. |
| `moto-cli` | bin | CLI commands: `moto garage {list,open,close}`, `moto bike {...}` |

### Club Crates

| Crate | Type | Purpose |
|-------|------|---------|
| `moto-club` | bin | Server main: wires together API, services |
| `moto-club-api` | lib | REST handlers for garage/bike operations |
| `moto-club-garage` | lib | Garage service: lifecycle state machine, TTL |
| `moto-club-k8s` | lib | K8s operations specific to club (wraps moto-k8s) |
| `moto-club-db` | lib | Database: migrations, repositories |
| `moto-club-wg` | lib | WireGuard coordination: IPAM, peers, sessions, DERP |
| `moto-club-reconcile` | lib | K8s → DB reconciliation loop |
| `moto-club-ws` | lib | WebSocket handlers for peer streaming |

### WireGuard Tunnel Crates

| Crate | Type | Purpose |
|-------|------|---------|
| `moto-wgtunnel-types` | lib | Keys, IPs, peers, DERP types |
| `moto-wgtunnel-conn` | lib | MagicConn, STUN, endpoint selection |
| `moto-wgtunnel-derp` | lib | DERP protocol, client, map |
| `moto-wgtunnel-engine` | lib | Tunnel management, platform TUN |
| `moto-cli-wgtunnel` | lib | CLI integration: enter command tunnel setup |
| `moto-garage-wgtunnel` | lib | Garage daemon: registration, health |

### Keybox Crates

| Crate | Type | Purpose |
|-------|------|---------|
| `moto-keybox` | lib | Core: SVID, ABAC, envelope encryption, repository |
| `moto-keybox-server` | bin | HTTP server: auth, secrets, audit endpoints |
| `moto-keybox-client` | lib | Client for garages/bikes: SVID cache, secret fetching |
| `moto-keybox-cli` | bin | Admin CLI: init, set/get secrets, issue dev SVIDs |
| `moto-keybox-db` | lib | Database: models, migrations, repositories |

### Test Crates

| Crate | Type | Purpose |
|-------|------|---------|
| `moto-test-utils` | lib | Test utilities: `test_pool()`, `unique_garage_name()`, `unique_owner()`, `fake_wg_pubkey()` |

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

Note: `GarageStatus` enum and `Garage` struct live in `moto-club-db`, not in `moto-club-types`. See [moto-club.md](moto-club.md) for the full Garage model.

**BikeId**, **BikeState**, **BikeInfo** (future) - Similar pattern for bikes

**API Types** (future) - `CreateGarageRequest`, `CreateGarageResponse`, `ListGaragesRequest`, etc.

### moto-k8s

**K8sClient** - Wraps `kube::Client`, provides namespace/pod operations

**NamespaceOps** - Trait: `create_namespace()`, `delete_namespace()`, `list_namespaces()`, `get_namespace()`

**Labels** - Constants: `moto.dev/type`, `moto.dev/garage-id`, `moto.dev/garage-name`, `moto.dev/bike-id`, `moto.dev/bike-name`, `moto.dev/owner`, `moto.dev/expires-at`, `moto.dev/engine`

### moto-garage

**DEPRECATED** - This crate should be removed. The CLI now talks directly to moto-club API using context configuration. There is no need for a separate abstraction layer.

~~**GarageMode** - Enum: `Local` (direct K8s) or `Remote { endpoint }` (via club)~~

~~**GarageClient** - Methods: `list()`, `open(name)`, `close(id)`~~

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

**DEPRECATED** - Local mode is no longer supported. The CLI always talks to moto-club API.

~~The CLI can work in two modes:~~
~~- **Local**: Direct K8s access via kubeconfig (for solo dev)~~
~~- **Remote**: Through moto-club server (for team/managed)~~

~~`moto-garage` abstracts this - same interface, different backend.~~

The CLI uses `--context` to select which moto-club instance to talk to. Even for local development, moto-club runs in the local cluster and the CLI connects to it via API.

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
| Garage | Dev environment | ~~moto-garage~~ (deprecated) |
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

### v1.7 (2026-03-05)
- Fix: `moto-club-types` only exports `GarageId`; `GarageStatus` and `Garage` live in `moto-club-db`. Remove stale `GarageState`/`GarageInfo` definitions from Key Types section.
- Fix: K8s labels use resource-specific names (`moto.dev/garage-id`, `moto.dev/garage-name`, `moto.dev/bike-id`, `moto.dev/bike-name`) not generic (`moto.dev/id`, `moto.dev/name`). Add `moto.dev/expires-at` and `moto.dev/engine`.

### v1.6 (2026-03-05)
- Fix: Remove `docker/` directory — container builds use Nix `dockerTools` in `infra/pkgs/`, not Dockerfiles
- Fix: Update `infra/` tree to match actual layout (`infra/modules/`, `infra/pkgs/`, `infra/k8s/moto-system/`) instead of outdated `infra/nix/` structure

### v1.5 (2026-02-24)
- Docs: Add `moto-club-ws` crate to Club Crates table and directory tree (re-introduced by moto-club.md v1.6)
- Docs: Add `moto-test-utils` crate to crate tables and directory tree (test utilities for integration tests)

### v1.4 (2026-02-04)

**BREAKING: Remove `moto-garage` crate entirely.**

The `moto-garage` crate provided a client library with local/remote modes, but is now dead code:
- CLI uses `moto-cli-wgtunnel` which talks directly to moto-club API
- No consumers exist in the workspace
- Remove from Cargo.toml workspace members
- Delete `crates/moto-garage/` directory

### v1.3 (2026-02-04)

Major crate documentation update:
- Remove `moto-club-ws` (doesn't exist, WebSocket is in moto-club-api)
- Remove `moto-club-bike` (doesn't exist, future work)
- Add WireGuard Tunnel Crates section (6 crates)
- Add Keybox Crates section (5 crates)
- Add `moto-club-wg` and `moto-club-reconcile` to Club Crates
- Update `moto-keybox` from "future" to implemented
- Remove `moto-garage` crate (per v1.2 deprecation)

### v1.2 (2026-02-03)

Deprecated `moto-garage` crate and local mode. The CLI now always talks to moto-club API using context configuration. Remove the following:
- `crates/moto-garage/` directory
- All references to `GarageClient` and `GarageMode` in moto-cli
- Local mode logic that bypasses moto-club API

### v1.1 (2026-01-21)

Renamed `moto-k3s` → `moto-k8s`. The crate is K8s-agnostic; k3s is an infrastructure choice, not a code dependency.

### v1.0 (2026-01-20)

Initial spec.
