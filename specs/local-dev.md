# Local Development

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Wrenching |
| Last Updated | 2026-02-19 |

## Overview

Run the full moto stack locally for development. The control plane (moto-club, moto-keybox) runs as local cargo processes, backed by a Docker Compose Postgres and the k3d cluster.

**This is the fastest path to a working system.** No K8s deployment of the control plane needed — just `cargo run`.

**Scope:**
- Docker Compose for development databases
- Keybox bootstrap (key generation)
- Running moto-club and moto-keybox locally
- Building and pushing the garage image
- Makefile targets for the local dev workflow

**Out of scope:**
- Production deployment (see [service-deploy.md](service-deploy.md))
- CI/CD pipelines
- Remote clusters

## Specification

### Prerequisites

| Requirement | Why |
|-------------|-----|
| Docker or Colima | Postgres, k3d, container builds |
| k3d | Local K8s cluster (see [local-cluster.md](local-cluster.md)) |
| Nix | Dev shell with Rust toolchain |

### Architecture

```
┌─────────────┐     ┌──────────────┐     ┌──────────────┐
│  moto CLI   │────>│  moto-club   │────>│  k3d cluster │
│ (cargo run) │     │ (cargo run)  │     │  (garages)   │
└─────────────┘     └──────┬───────┘     └──────────────┘
                           │
                    ┌──────┴───────┐
                    │ moto-keybox  │
                    │ (cargo run)  │
                    └──────┬───────┘
                           │
                    ┌──────┴───────┐
                    │  PostgreSQL  │
                    │  (docker)    │
                    └──────────────┘
```

All services run as local processes. Only garages run in K8s.

### Development Database

A `docker-compose.yml` provides a single Postgres instance with two databases:

| Database | Used by | Migrations |
|----------|---------|------------|
| `moto_club` | moto-club (garages, WireGuard state) | Manual (`sqlx migrate run`) |
| `moto_keybox` | moto-keybox (secrets, audit logs) | Automatic (on server startup) |

| Property | Value |
|----------|-------|
| Image | `postgres:16-alpine` |
| Port | 5432 |
| Credentials | `moto` / `moto` |
| Persistence | Named volume for pgdata |
| Healthcheck | `pg_isready` |

**Note:** This is separate from `docker-compose.test.yml` which runs on port 5433 for integration tests.

### Keybox Bootstrap

Before first run, cryptographic keys must be generated and stored in `.dev/keybox/`:

| File | Contents |
|------|----------|
| `master.key` | AES-256 KEK (base64-encoded) |
| `signing.key` | Ed25519 SVID signing key (base64-encoded) |
| `service-token` | Static hex token for moto-club → keybox auth |

The `.dev/` directory is gitignored. Keys are generated once and reused across dev sessions. The `moto keybox init` CLI command can generate these, or they can be generated manually.

### Port Assignments

Both servers default to port 8080, so local dev must use different ports:

| Service | API Port | Health Port | Metrics Port |
|---------|----------|-------------|--------------|
| moto-keybox | 8090 | 8091 | — |
| moto-club | 8080 | 8081 | 9090 |

### Environment Variables

**moto-keybox-server:**

| Variable | Dev Value |
|----------|-----------|
| `MOTO_KEYBOX_BIND_ADDR` | `0.0.0.0:8090` |
| `MOTO_KEYBOX_HEALTH_BIND_ADDR` | `0.0.0.0:8091` |
| `MOTO_KEYBOX_MASTER_KEY_FILE` | `.dev/keybox/master.key` |
| `MOTO_KEYBOX_SVID_SIGNING_KEY_FILE` | `.dev/keybox/signing.key` |
| `MOTO_KEYBOX_DATABASE_URL` | `postgres://moto:moto@localhost:5432/moto_keybox` |
| `MOTO_KEYBOX_SERVICE_TOKEN_FILE` | `.dev/keybox/service-token` |
| `RUST_LOG` | `moto_keybox=debug` |

**moto-club:**

| Variable | Dev Value |
|----------|-----------|
| `MOTO_CLUB_DATABASE_URL` | `postgres://moto:moto@localhost:5432/moto_club` |
| `MOTO_CLUB_KEYBOX_URL` | `http://localhost:8090` |
| `MOTO_CLUB_DEV_CONTAINER_IMAGE` | `localhost:5000/moto-garage:latest` |
| `RUST_LOG` | `moto_club=debug` |

K8s access comes from `~/.kube/config` (the `k3d-moto` context created by `moto cluster init`).

### Garage Image

The garage dev container image must be built and available in the k3d cluster's local registry (`localhost:5000`) before creating garages.

### Startup Sequence

Full sequence from zero to working:

```bash
# 1. Create k3d cluster (idempotent)
moto cluster init

# 2. Start development database
make dev-db-up

# 3. Generate keybox keys (first time only)
make dev-keybox-init

# 4. Run database migrations
make dev-db-migrate

# 5. Build and push garage image (slow first time)
make dev-garage-image

# 6. Start keybox (terminal 1)
make dev-keybox

# 7. Start moto-club (terminal 2)
make dev-club

# 8. Open a garage (terminal 3)
MOTO_USER=nick moto garage open --no-attach
```

### Shortcut

`make dev-up` runs steps 2-7 automatically. Starts moto-club in foreground — Ctrl-C stops everything.

### Teardown

| Target | Behavior |
|--------|----------|
| `dev-down` | Stop all services and postgres |
| `dev-clean` | dev-down + remove pgdata volume + remove `.dev/` |

### Makefile Targets

| Target | Description |
|--------|-------------|
| `dev-up` | Start full local dev stack |
| `dev-down` | Stop all services and database |
| `dev-clean` | dev-down + remove all dev state |
| `dev-db-up` | Start postgres only |
| `dev-db-down` | Stop postgres |
| `dev-db-migrate` | Run moto-club-db migrations |
| `dev-keybox-init` | Generate keybox keys in `.dev/` |
| `dev-keybox` | Start moto-keybox-server |
| `dev-club` | Start moto-club |
| `dev-garage-image` | Build and push garage image to local registry |

### File Layout

```
moto/
├── docker-compose.yml          # Dev databases (port 5432)
├── docker-compose.test.yml     # Test database (port 5433)
├── scripts/
│   └── init-dev-db.sql         # Creates moto_keybox database
├── .dev/                       # Local dev state (gitignored)
│   └── keybox/
│       ├── master.key
│       ├── signing.key
│       └── service-token
└── .gitignore                  # Must include .dev/
```

### Troubleshooting

| Problem | Solution |
|---------|----------|
| Port 5432 in use | Stop local Postgres or change port in docker-compose.yml |
| Port 8080/8090 in use | Check for other services on those ports |
| `moto-club` can't reach K8s | Run `moto cluster init` |
| Garage pod `ImagePullBackOff` | Run `make dev-garage-image` |
| Keybox key errors | Delete `.dev/keybox/` and run `make dev-keybox-init` |
| Migration errors | Check postgres is running: `docker compose ps` |

## Deferred

- Automatic DERP relay server for WireGuard NAT traversal
- Hot reload / watch mode for moto-club and keybox
- Pre-seeding secrets in keybox for private repo cloning

## References

- [local-cluster.md](local-cluster.md) — k3d cluster setup
- [makefile.md](makefile.md) — Makefile targets
- [keybox.md](keybox.md) — Keybox server configuration
- [moto-club.md](moto-club.md) — moto-club server configuration
- [container-system.md](container-system.md) — Image build pipeline
- [service-deploy.md](service-deploy.md) — K8s deployment (alternative)

## Changelog

### v0.1 (2026-02-19)
- Initial specification
