# Moto CLI

| | |
|--------|----------------------------------------------|
| Version | 0.8 |
| Status | Ready to Rip |
| Last Updated | 2026-02-27 |

## Overview

The `moto` command-line interface for managing garages (dev environments) and bikes (deployed services).

**Design principles:**
- **Minimal surface area** - Only essential commands for v1
- **Noun-first structure** - `moto <noun> <verb>`
- **Human-friendly by default** - Pretty output, `--json` for scripting

## Command Hierarchy

```
moto
├── garage
│   ├── open        # Create a new garage
│   ├── enter       # Connect to garage terminal
│   ├── logs        # View garage logs
│   ├── list        # List garages
│   └── close       # Tear down a garage
├── bike
│   ├── build       # Build container image
│   ├── deploy      # Deploy a bike
│   ├── list        # List bikes
│   └── logs        # View bike logs
├── dev
│   ├── up          # Start local dev stack
│   ├── down        # Stop local dev stack
│   └── status      # Dev environment health check
└── cluster
    ├── init        # Bootstrap cluster
    └── status      # Cluster health check
```

**Deferred to future versions:** bike validate/stop/scale/history/rollback, secret management, cluster upgrade/backup/restore, shell completions.

---

## Identifiers

**Garage names** are auto-generated (e.g., `bold-mongoose`) and serve as the unique identifier. All commands accept the garage name.

**Bike names** come from `bike.toml` and must be unique within a context.

---

## Global Flags

| Flag | Short | Description |
|------|-------|-------------|
| `--json` | `-j` | Output in JSON format |
| `--verbose` | `-v` | Increase output verbosity |
| `--quiet` | `-q` | Suppress non-essential output |
| `--context <name>` | `-c` | Override kubectl context |
| `--help` | `-h` | Show help |
| `--version` | `-V` | Show moto version |

---

## Configuration

### Location

```
$XDG_CONFIG_HOME/moto/config.toml
# Falls back to: ~/.config/moto/config.toml
```

### Format

```toml
[output]
color = "auto"  # auto, always, never

[garage]
ttl = "4h"      # default TTL
```

### Configuration Precedence

1. Command-line flags
2. Environment variables
3. Config file
4. Defaults

### Environment Variables

| Variable | Description |
|----------|-------------|
| `MOTOCONFIG` | Override kubeconfig (falls back to `KUBECONFIG`) |
| `MOTO_NO_COLOR` | Disable colored output |
| `MOTO_JSON` | Force JSON output |

---

## Command Reference

### Garage Commands

#### `moto garage open`

Creates a new garage. Names are auto-generated.

```
Usage: moto garage open [options]

Options:
  --engine <name>   Engine to work on (default: current directory name)
  --ttl <duration>  Time-to-live (default: 4h, max: 48h)
  --owner <name>    Owner of the garage (default: current user)
```

**Duration format:** `<number><unit>` where unit is `m` (minutes), `h` (hours), or `d` (days).

**Example:**
```
$ moto garage open
Garage created: abc123
  Name:    bold-mongoose
  Branch:  main
  TTL:     4h (expires 2026-01-20 02:48:00)
  Status:  running

To connect: moto garage enter bold-mongoose
```

**JSON output:**
```json
{
  "id": "abc123",
  "name": "bold-mongoose",
  "branch": "main",
  "ttl_seconds": 14400,
  "expires_at": "2026-01-20T02:48:00Z",
  "status": "running"
}
```

**Exit codes:** 0 success, 1 error

---

#### `moto garage enter`

Connects to a garage terminal session.

```
Usage: moto garage enter <name> [options]

Options:
  --kubectl    Connect via kubectl exec instead of WireGuard tunnel
```

**Connection modes:**
- **Default (no flag):** WireGuard tunnel (existing behavior, unchanged).
- **`--kubectl`:** Skips WireGuard entirely. Connects via `kubectl exec -it -n {namespace} {pod_name} -- tmux attach-session -t garage`. Useful for local dev before the garage pod's WireGuard daemon is running.

`namespace` and `pod_name` come from the `get_garage` API response (falling back to `moto-garage-{id[..8]}` and `dev-container` if empty).

The `--kubectl` flag also works on `garage open` (when `--no-attach` is not set) to attach via kubectl after creation.

**Example (default, WireGuard):**
```
$ moto garage enter bold-mongoose
Connecting to bold-mongoose...
[garage: bold-mongoose] $
```

**Example (kubectl):**
```
$ moto garage enter bold-mongoose --kubectl
Connecting to bold-mongoose via kubectl...
[garage: bold-mongoose] $
```

Use `Ctrl+P, Ctrl+Q` to detach. Garage keeps running.

**Exit codes:** 0 normal exit, 1 connection failed, 2 not found

---

#### `moto garage logs`

View logs from a garage.

```
Usage: moto garage logs <name> [options]

Options:
  --follow, -f    Stream logs continuously
  --tail, -n <n>  Show last n lines (default: 100)
  --since <dur>   Show logs from last duration (e.g., 5m, 1h)
```

**Note:** `--since` is a relative duration, not an absolute time. `--since 5m` means "logs from the last 5 minutes".

**Example:**
```
$ moto garage logs bold-mongoose -f
2026-01-21T10:15:32Z Starting dev environment...
2026-01-21T10:15:33Z Claude Code ready
```

**Exit codes:** 0 success, 1 error, 2 not found

---

#### `moto garage list`

Lists garages.

```
Usage: moto garage list [options]

Options:
  --context <name>   Filter by kubectl context (use "all" for all contexts)
```

**Example:**
```
$ moto garage list
ID       NAME            BRANCH   STATUS    TTL       AGE
abc123   bold-mongoose   main     running   1h45m     2h15m
def456   quiet-falcon    main     running   3h15m     45m
```

**JSON output:**
```json
{
  "garages": [
    {
      "id": "abc123",
      "name": "bold-mongoose",
      "branch": "main",
      "status": "running",
      "ttl_remaining_seconds": 6300,
      "age_seconds": 8100
    }
  ]
}
```

**Exit codes:** 0 success, 1 error

---

#### `moto garage close`

Tears down a garage.

```
Usage: moto garage close <name> [options]

Options:
  --force    Skip confirmation
```

**JSON output:**
```json
{
  "name": "bold-mongoose",
  "status": "closed"
}
```

**Exit codes:** 0 closed, 1 error, 2 not found

---

### Bike Commands

#### bike.toml

Bike commands look for `bike.toml` in the current working directory. If not found, they search up to the git root.

See [moto-bike.md](moto-bike.md) for the full `bike.toml` specification.

---

#### `moto bike build`

Builds container image from `bike.toml` in current directory.

```
Usage: moto bike build [options]

Options:
  --tag <tag>    Override image tag (default: git sha)
  --push         Push to registry after build
```

**Example:**
```
$ moto bike build
Building api-service...
Build complete: api-service:abc123f
```

**JSON output:**
```json
{
  "name": "api-service",
  "image": "api-service:abc123f",
  "pushed": false
}
```

**Exit codes:** 0 success, 1 build failed, 2 bike.toml not found

---

#### `moto bike deploy`

Deploys a bike to the current context.

```
Usage: moto bike deploy [options]

Options:
  --image <tag>       Deploy specific image (default: latest local build)
  --replicas <n>      Override replica count
  --wait              Wait for deployment to complete
  --wait-timeout <d>  Timeout for --wait (default: 5m)
```

**Note:** If no `--image` specified and no local build exists, the command fails.

**Example:**
```
$ moto bike deploy --wait
Deploying api-service:abc123f...
  Waiting for pods... 2/2 ready
Deployment complete.
```

**JSON output:**
```json
{
  "name": "api-service",
  "image": "api-service:abc123f",
  "replicas": 2,
  "status": "deployed"
}
```

**Exit codes:** 0 success, 1 deploy failed, 2 bike.toml not found, 3 image not found

---

#### `moto bike list`

Lists bikes in the current context.

```
Usage: moto bike list
```

**Example:**
```
$ moto bike list
NAME          STATUS    REPLICAS   AGE     IMAGE
api-service   running   2/2        3d      api-service:abc123f
worker        running   1/1        1d      worker:def456a
```

**JSON output:**
```json
{
  "bikes": [
    {
      "name": "api-service",
      "status": "running",
      "replicas_ready": 2,
      "replicas_desired": 2,
      "age_seconds": 259200,
      "image": "api-service:abc123f"
    }
  ]
}
```

**Exit codes:** 0 success, 1 error

---

#### `moto bike logs`

View logs from a bike.

```
Usage: moto bike logs <name> [options]

Options:
  --follow, -f    Stream logs continuously (Ctrl+C to stop)
  --tail <n>      Show last n lines (default: 100)
  --since <dur>   Show logs from last duration (e.g., 5m, 1h)
```

**Note:** `--since` is a relative duration. `--since 1h` means "logs from the last hour".

**Example:**
```
$ moto bike logs api-service -f
2026-01-21T10:15:32Z INFO Started on :8080
2026-01-21T10:15:33Z INFO Health check passed
```

**Exit codes:** 0 success, 1 error, 2 not found

---

### Cluster Commands

#### `moto cluster init`

Bootstraps a local k3s cluster.

```
Usage: moto cluster init
```

**Example:**
```
$ moto cluster init
Initializing local k3s cluster...
  Installing k3s...
  Starting cluster...
Cluster ready.
```

**JSON output:**
```json
{
  "name": "moto",
  "type": "k3d",
  "status": "created"
}
```

**Exit codes:** 0 success, 1 failed

---

#### `moto cluster status`

Shows cluster health.

```
Usage: moto cluster status
```

**Example:**
```
$ moto cluster status
Cluster: local (k3s)
Status: healthy

Components:
  moto-club:  running
  keybox:     running
  postgres:   running
```

**JSON output:**
```json
{
  "cluster": {
    "name": "local",
    "type": "k3s",
    "status": "healthy"
  },
  "components": [
    {"name": "moto-club", "status": "running"},
    {"name": "keybox", "status": "running"}
  ]
}
```

**Exit codes:** 0 healthy, 1 unhealthy

---

### Dev Commands

#### `moto dev up`

Starts the full local dev stack: cluster, postgres, keybox, moto-club, and optionally opens a garage. Runs in foreground — Ctrl-C stops everything.

See [local-dev.md](local-dev.md) for the full specification.

```
Usage: moto dev up [options]

Options:
  --no-garage       Start services only, don't open a garage
  --rebuild-image   Force rebuild and push the garage container image
  --skip-image      Skip the registry image check entirely
```

**Example:**
```
$ moto dev up
[1/9] Checking prerequisites...     ok
[2/9] Ensuring cluster...           exists
[3/9] Checking garage image...      found in registry
[4/9] Starting postgres...          ready (localhost:5432)
[5/9] Generating keybox keys...     found (.dev/keybox/)
[6/9] Running migrations...         up to date
[7/9] Starting keybox...            healthy (localhost:8090)
[8/9] Starting moto-club...         healthy (localhost:8080)
[9/9] Opening garage...             bold-mongoose

moto dev environment ready!

  Postgres:  localhost:5432
  Keybox:    localhost:8090
  Club:      localhost:8080
  Garage:    bold-mongoose

  To connect: moto garage enter bold-mongoose
  To stop:    Ctrl-C
```

**JSON output:**
```json
{
  "cluster": "running",
  "postgres": "healthy",
  "keybox": "healthy",
  "club": "healthy",
  "garage": "bold-mongoose"
}
```

**Exit codes:** 0 success (on Ctrl-C), 1 error

---

#### `moto dev down`

Stops running dev services and postgres.

```
Usage: moto dev down [options]

Options:
  --clean    Also remove .dev/ directory and postgres data volume
```

**Example:**
```
$ moto dev down
Stopping moto-club... done
Stopping keybox... done
Stopping postgres... done
```

**Exit codes:** 0 success, 1 error

---

#### `moto dev status`

Shows health of all local dev components.

```
Usage: moto dev status
```

**Example:**
```
$ moto dev status
Cluster:   running (k3d-moto)
Registry:  healthy (localhost:5050)
Postgres:  healthy (localhost:5432)
Keybox:    healthy (localhost:8090)
Club:      healthy (localhost:8080)
Image:     moto-garage:latest (in registry)
Garages:   1 running
```

**JSON output:**
```json
{
  "cluster": "running",
  "registry": "healthy",
  "postgres": "healthy",
  "keybox": "healthy",
  "club": "healthy",
  "image": "found",
  "garages": 1
}
```

**Exit codes:** 0 all healthy, 1 any component unhealthy

---

## Error Handling

Errors include actionable suggestions:

```
$ moto garage enter missing-garage
Error: Garage 'missing-garage' not found.

Try: moto garage list
```

```
$ moto bike build
Error: No bike.toml found in current directory or parent directories.

Try: Create a bike.toml or cd to a directory containing one.
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Resource not found |
| 3 | Invalid input (e.g., image not found) |
| 130 | Interrupted (Ctrl+C) |

---

## Implementation Notes

- CLI written in Rust using `clap`
- Communicates with moto-club REST API
- K8s operations via `kube` crate
- Garage connectivity via WireGuard

## References

- [jj-workflow.md](jj-workflow.md) - Code sync and PR workflow
- [moto-wgtunnel.md](moto-wgtunnel.md) - WireGuard connectivity
- [garage-lifecycle.md](garage-lifecycle.md) - Garage state machine
- [moto-bike.md](moto-bike.md) - Bike base image and engine contract
- [moto-club.md](moto-club.md) - Server API

---

## Changelog

### v0.8 (2026-02-27)
- Add `--kubectl` flag to `garage enter`: connects via `kubectl exec -it -n {namespace} {pod_name} -- tmux attach-session -t garage` instead of WireGuard tunnel
- Add `--kubectl` flag to `garage open`: same kubectl attach path when `--no-attach` is not set
- `get_garage` API response provides namespace and pod_name; defaults to `moto-garage-{id[..8]}` / `dev-container` if empty
- WireGuard path is unchanged; `--kubectl` is a separate, explicit mode

### v0.7 (2026-02-24)
- Docs: Update `garage open` output format and JSON to match garage-lifecycle.md v0.4 (add id, branch, expires_at; remove engine)
- Docs: Update `garage list` table columns to match garage-lifecycle.md v0.4 (ID, NAME, BRANCH, STATUS, TTL, AGE)
- Docs: Update `garage list` JSON to match garage-lifecycle.md v0.4 (add id, branch; remove engine)

### v0.6 (2026-02-24)
- Add `dev` subcommand to command hierarchy: `up`, `down`, `status`
- Add Dev Commands reference section with usage, examples, JSON output, exit codes

### v0.5
- Fix: `garage list --context <name>` must actually filter results by context (v0.4 fix validated context name but did not filter the API response)
- Fix: `garage logs` must respect `--context` global flag when creating K8s client (currently always uses default kubectl context)
- Fix: `cluster init --json` must include `type` field per spec (currently omitted from output)
- Fix: `cluster init --json` example corrected: `"name": "moto"`, `"type": "k3d"`, `"status": "created"` (was stale `"local"`, `"k3s"`, `"ready"`)

### v0.4
- Fix: Implement `garage logs` command (currently returns error directing to kubectl)
- Fix: `--owner` flag must be passed to API (currently parsed but ignored)
- Fix: `--context` filtering for `garage list` (currently not filtering)
- Add: `--branch` flag to `garage open` (pass to API, default to "main")
- Add: `--no-attach` flag to `garage open` (create without connecting)

### v0.3
- Fixed detach key sequence: `Ctrl+P, Ctrl+Q` (was incorrectly `Ctrl+D`)
- Removed `sync` and `rebase` commands (agents use jj/gh directly, see jj-workflow.md)

### v0.2
- Added `--owner` flag to `garage open`
- Added `-n` short flag for `--tail` in `garage logs`
- Clarification: these were already implemented, spec updated to match
