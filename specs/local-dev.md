# Local Development

| | |
|--------|----------------------------------------------|
| Version | 0.11 |
| Status | Ripping |
| Last Updated | 2026-03-10 |

## Overview

Run the full moto stack locally for development. The control plane (moto-club, moto-keybox, moto-ai-proxy) runs as local cargo processes, backed by a Docker Compose Postgres and the k3d cluster.

**This is the fastest path to a working system.** No K8s deployment of the control plane needed — just `cargo run`.

**Scope:**
- Docker Compose for development databases
- Keybox bootstrap (key generation)
- Running moto-club, moto-keybox, and moto-ai-proxy locally
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
| Rust toolchain | Building moto binaries locally |

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

Before first run, cryptographic keys must be generated and stored in `.dev/keybox/`. See [keybox.md](keybox.md) for key formats and the `moto-keybox` CLI.

| File | Contents |
|------|----------|
| `master.key` | AES-256 KEK (base64-encoded) |
| `signing.key` | Ed25519 SVID signing key (base64-encoded) |
| `service-token` | Static hex token for moto-club → keybox auth |

The `.dev/` directory is gitignored. Keys are generated once and reused across dev sessions. Note: `moto-keybox init` generates `master.key` and `signing.key` but not `service-token` — the `dev-keybox-init` target must generate all three.

### Port Assignments

Local dev uses distinct ports per service to avoid collisions. Port 18080 matches the k3d deploy path (see [service-deploy.md](service-deploy.md)), so the CLI default works for both modes.

| Service | API Port | Health Port | Metrics Port |
|---------|----------|-------------|--------------|
| moto-keybox | 8090 | 8091 | — |
| moto-club | 18080 | 8081 | 9090 |
| moto-ai-proxy | 18090 | 18091 | — |

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
| `MOTO_CLUB_BIND_ADDR` | `0.0.0.0:18080` |
| `MOTO_CLUB_DATABASE_URL` | `postgres://moto:moto@localhost:5432/moto_club` |
| `MOTO_CLUB_KEYBOX_URL` | `http://localhost:8090` |
| `MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE` | `.dev/keybox/service-token` |
| `MOTO_CLUB_KEYBOX_HEALTH_URL` | `http://localhost:8091` |
| `MOTO_CLUB_DEV_CONTAINER_IMAGE` | `moto-registry:5000/moto-garage:latest` |
| `RUST_LOG` | `moto_club=debug` |

**moto-ai-proxy:**

| Variable | Dev Value |
|----------|-----------|
| `MOTO_AI_PROXY_BIND_ADDR` | `0.0.0.0:18090` |
| `MOTO_AI_PROXY_HEALTH_BIND_ADDR` | `0.0.0.0:18091` |
| `MOTO_AI_PROXY_KEYBOX_URL` | `http://localhost:8090` |
| `MOTO_AI_PROXY_SVID_FILE` | `.dev/ai-proxy/svid.jwt` |
| `MOTO_AI_PROXY_CLUB_URL` | `http://localhost:18080` |
| `MOTO_AI_PROXY_KEY_CACHE_TTL_SECS` | `300` |
| `MOTO_AI_PROXY_GARAGE_CACHE_TTL_SECS` | `60` |
| `RUST_LOG` | `moto_ai_proxy=debug` |

K8s access comes from `~/.kube/config` (the `k3d-moto` context created by `moto cluster init`).

### Garage Image

The garage dev container image must be built and pushed to the k3d registry (`localhost:5050` from the host, `moto-registry:5000` from inside k3d). The `MOTO_CLUB_DEV_CONTAINER_IMAGE` must use the in-cluster registry name (`moto-registry:5000`) since pods pull images from inside k3d, not from the host.

`make push-garage` cleans up the local Docker daemon copy after pushing to the registry. The image only needs to live in the registry (for k3d to pull) — keeping it in Docker wastes ~10GB of VM disk.

### `moto dev` — One Command Local Dev

The `moto dev` subcommand orchestrates the full local dev flow. One command, one terminal:

```bash
moto dev up
```

This performs all steps: cluster check, postgres, keybox keys, migrations, starts keybox, club, and ai-proxy as background subprocesses, and opens a garage. Ctrl-C tears everything down.

#### `moto dev up`

```
moto dev up [--no-garage] [--rebuild-image] [--skip-image] [--no-ai-proxy]
```

| Flag | Behavior |
|------|----------|
| `--no-garage` | Start services only, don't open a garage |
| `--rebuild-image` | Force rebuild and push the garage container image |
| `--skip-image` | Skip the registry image check entirely |
| `--no-ai-proxy` | Skip starting moto-ai-proxy |

Orchestration steps (each idempotent, with progress output):

```
[1/10] Checking prerequisites...     docker, k3d, MOTO_USER
[2/10] Ensuring cluster...          exists / created
[3/10] Checking garage image...     found in registry / missing
[4/10] Starting postgres...         ready (localhost:5432)
[5/10] Generating keybox keys...    found (.dev/keybox/) / generated
[6/10] Running migrations...        up to date
[7/10] Starting keybox...            healthy (localhost:8090)
[8/10] Starting moto-club...         healthy (localhost:18080)
[9/10] Starting ai-proxy...          healthy (localhost:18090)
[10/10] Opening garage...            bold-mongoose
```

**Step details:**

| Step | What it checks / does | On failure |
|------|----------------------|------------|
| 1. Prerequisites | Docker running, k3d installed, `MOTO_USER` set (falls back to `whoami`). With `--no-garage`, MOTO_USER is not required. | Abort with actionable error message |
| 2. Cluster | If k3d cluster `moto` exists, skip. If not, create it (same as `moto cluster init`). | Abort |
| 3. Image | `GET http://localhost:5050/v2/moto-garage/tags/list` — if response contains a `latest` tag, skip. If missing or registry unreachable: with `--rebuild-image`, run the Docker build + push inline; otherwise abort with instructions to run `make dev-garage-image`. With `--skip-image`, skip entirely. `--skip-image` and `--rebuild-image` together is invalid (abort with error). | Abort (unless `--skip-image`) |
| 4. Postgres | Run `docker compose up -d --wait`. Idempotent — no-op if already running. | Abort |
| 5. Keys | Check if all three files exist in `.dev/keybox/` (`master.key`, `signing.key`, `service-token`). If any missing, regenerate all: run `moto-keybox init --output-dir .dev/keybox --force`, then generate service-token (`openssl rand -hex 32`), set permissions to 600. | Abort |
| 6. Migrations | Run `cargo sqlx migrate run --source crates/moto-club-db/migrations` against the club database. Keybox migrations run automatically on keybox startup (step 7). | Abort |
| 7. Keybox | Spawn `moto-keybox-server` as subprocess with dev env vars. Wait for `GET http://localhost:8091/health/ready` to return 200. Timeout: 30 seconds with exponential backoff. | Abort (kill keybox subprocess) |
| 8. Club | Spawn `moto-club` as subprocess with dev env vars. Wait for `GET http://localhost:8081/health/ready` to return 200. Timeout: 30 seconds with exponential backoff. | Abort (kill all subprocesses) |
| 9. AI Proxy | Spawn `moto-ai-proxy` as subprocess with dev env vars. Wait for `GET http://localhost:18091/health/ready` to return 200. Timeout: 30 seconds with exponential backoff. Skipped with `--no-ai-proxy`. | Abort (kill all subprocesses) |
| 10. Garage | `POST http://localhost:18080/api/v1/garages` with auto-generated name and default TTL. Skipped with `--no-garage`. | Print warning, continue (services are still running) |

**Failure behavior:** Steps 1-9 abort on failure. Step 10 is best-effort — if garage creation fails, services keep running and the user can open a garage manually. On abort, any already-started subprocesses are killed and postgres is left running (fast restart).

**Image build inline:** When `--rebuild-image` triggers a build, it runs the same Docker build as `make build-garage` + `make push-garage`, with progress output. This can take 15-20 minutes on first run.

**Subprocess management:** Keybox, club, and ai-proxy are spawned as subprocesses. On Ctrl-C (SIGINT), all subprocesses are killed. Postgres is left running (fast restart on next `dev up`). Exit code is 0 on Ctrl-C. If any subprocess dies unexpectedly, an error is printed and all other subprocesses are killed.

**Subprocess output:** Log output from keybox, club, and ai-proxy is suppressed by default. With `-v`, subprocess stderr is forwarded to the terminal. With `-vv`, both stdout and stderr are forwarded.

**DevConfig:** All env vars from the tables above use hardcoded dev defaults. Each is overridable via the same environment variable name (e.g., set `MOTO_KEYBOX_BIND_ADDR=0.0.0.0:9090` to override the default `0.0.0.0:8090`). Total: 22 env vars (7 keybox + 7 club + 8 ai-proxy).

**Idempotency:** Running `moto dev up` while services are already running restarts the services (kills existing subprocesses, starts new ones). Cluster, postgres, keys, and migrations are all idempotent checks that skip if already done.

#### `moto dev down`

```
moto dev down [--clean]
```

| Flag | Behavior |
|------|----------|
| (none) | Stop club, keybox, and postgres |
| `--clean` | Also remove `.dev/` directory and pgdata docker volume |

Stops services by:
1. Finding processes listening on ports 18080 (club) and 8090 (keybox) and sending SIGTERM
2. Running `docker compose down` to stop postgres (or `docker compose down -v` with `--clean`)
3. With `--clean`: removing `.dev/` directory

Running garages in k3d are not affected by `dev down`. The k3d cluster stays running.

#### `moto dev status`

```
moto dev status
```

Health-check dashboard:

```
Cluster:   running (k3d-moto)
Registry:  healthy (localhost:5050)
Postgres:  healthy (localhost:5432)
Keybox:    healthy (localhost:8090)
Club:      healthy (localhost:18080)
Image:     moto-garage:latest (in registry)
Garages:   1 running
```

**Health check methods:**

| Component | How checked |
|-----------|------------|
| Cluster | `k3d cluster list` — check if `moto` cluster exists and running |
| Registry | `GET http://localhost:5050/v2/` — returns 200 |
| Postgres | `docker compose ps` — check container is running and healthy |
| Keybox | `GET http://localhost:8091/health/ready` — returns 200 |
| Club | `GET http://localhost:8081/health/ready` — returns 200 |
| AI Proxy | `GET http://localhost:18091/health/ready` — returns 200 |
| Image | `GET http://localhost:5050/v2/moto-garage/tags/list` — contains `latest` tag |
| Garages | `GET http://localhost:18080/api/v1/garages` — count non-terminated garages |

### Manual Startup Sequence (Advanced)

For running services individually or debugging, the manual steps are still available:

```bash
# 1. Create k3d cluster (idempotent)
make dev-cluster

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

### Teardown

| Command | Behavior |
|---------|----------|
| `moto dev down` | Stop club, keybox, and postgres |
| `moto dev down --clean` | Stop everything + remove `.dev/` + pgdata volume |
| `make dev-down` | Stop postgres only |
| `make dev-clean` | dev-down + remove pgdata volume + `.dev/` |

### Makefile Targets

| Target | Description |
|--------|-------------|
| `dev` | Alias for `moto dev up` |
| `dev-cluster` | Create k3d cluster (idempotent, see [local-cluster.md](local-cluster.md)) |
| `dev-up` | Start full local dev stack (legacy, use `moto dev up` instead) |
| `dev-down` | Stop postgres only |
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
| Port 18080/8090 in use | Check for other services on those ports |
| `moto-club` can't reach K8s | Run `make dev-cluster` |
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

### v0.11 (2026-03-10)
- Add moto-ai-proxy as third service in `moto dev up` flow (step 9/10)
- Add `--no-ai-proxy` flag to skip ai-proxy startup
- Add ai-proxy port assignments (18090 API, 18091 health)
- Add 8 ai-proxy env vars to Environment Variables section
- Update step count from 9 to 10, subprocess management to include ai-proxy
- Total env vars: 22 (7 keybox + 7 club + 8 ai-proxy)

### v0.10 (2026-02-26)
- Change moto-club API port from 8080 to 18080 — matches k3d deploy path so CLI default works for both modes
- Add `MOTO_CLUB_BIND_ADDR` to moto-club env vars (overrides server default of 8080)

### v0.9 (2026-02-24)
- Docs: Fix `dev-down` description in Makefile Targets table (stops postgres only, not all services)

### v0.8 (2026-02-24)
- Clarify `moto dev up` step details: what each step checks, what happens on failure, abort vs continue
- Specify health check endpoints: keybox `/health/ready` on :8091, club `/health/ready` on :8081
- Specify health check timeout: 30 seconds with exponential backoff
- Clarify key generation: all three files regenerated if any missing, permissions set to 600
- Clarify Ctrl-C behavior: kill subprocesses, leave postgres running, exit code 0
- Clarify idempotency: running `dev up` twice restarts services
- Clarify subprocess output: suppressed by default, `-v` shows stderr, `-vv` shows all
- Specify flag validation: `--skip-image` + `--rebuild-image` is invalid
- Specify `dev down` behavior: SIGTERM to port processes, docker compose down, k3d unaffected
- Specify `dev status` health check methods for each component
- Fix env var count: 13 (was 12), add RUST_LOG for both services
- Step 9 (garage open) is best-effort: failure prints warning but services stay running

### v0.7 (2026-02-24)
- Add `moto dev` subcommand: `up`, `down`, `status` — one command to start the full local dev stack
- DevConfig: hardcoded dev defaults for all env vars, overridable per-variable
- Add `dev` Makefile target as alias for `moto dev up`

### v0.6 (2026-02-22)
- `push-garage` now cleans up local Docker daemon copy after pushing to registry (saves ~10GB VM disk)

### v0.5 (2026-02-21)
- Fix `MOTO_CLUB_DEV_CONTAINER_IMAGE` to use `moto-registry:5000` (in-cluster k3d registry name) instead of `localhost:5000` (host-only). Pods inside k3d can't reach `localhost:5000`.
- Update host registry push address from `localhost:5000` to `localhost:5050` (matches local-cluster.md v0.3 port change)

### v0.4 (2026-02-21)
- Add `MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE` to moto-club env vars (same service-token file used by keybox, needed for garage SVID issuance)

### v0.3 (2026-02-20)
- Add `MOTO_CLUB_KEYBOX_HEALTH_URL` to moto-club env vars (keybox health port differs in local dev)
- `dev-up` no longer rebuilds the garage image on every run — `dev-garage-image` is a one-time setup step

### v0.2 (2026-02-20)
- Add `dev-cluster` Makefile target for k3d cluster creation (was using bare `moto cluster init` CLI)

### v0.1 (2026-02-19)
- Initial specification
