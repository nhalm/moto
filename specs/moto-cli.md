# Moto CLI

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Ready to Rip |
| Last Updated | 2026-01-21 |

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
```

**Duration format:** `<number><unit>` where unit is `m` (minutes), `h` (hours), or `d` (days).

**Example:**
```
$ moto garage open
Created garage: bold-mongoose
  Engine: moto-club
  TTL: 4h

To connect: moto garage enter bold-mongoose
```

**JSON output:**
```json
{
  "name": "bold-mongoose",
  "engine": "moto-club",
  "ttl_seconds": 14400,
  "status": "running"
}
```

**Exit codes:** 0 success, 1 error

---

#### `moto garage enter`

Connects to a garage terminal session.

```
Usage: moto garage enter <name>
```

**Example:**
```
$ moto garage enter bold-mongoose
Connecting to bold-mongoose...
[garage: bold-mongoose] $
```

Use `Ctrl+D` to disconnect. Garage keeps running.

**Exit codes:** 0 normal exit, 1 connection failed, 2 not found

---

#### `moto garage logs`

View logs from a garage.

```
Usage: moto garage logs <name> [options]

Options:
  --follow, -f    Stream logs continuously
  --tail <n>      Show last n lines (default: 100)
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
NAME            STATUS    AGE     TTL       ENGINE
bold-mongoose   running   2h15m   1h45m     moto-club
quiet-falcon    running   45m     3h15m     keybox
```

**JSON output:**
```json
{
  "garages": [
    {
      "name": "bold-mongoose",
      "status": "running",
      "age_seconds": 8100,
      "ttl_remaining_seconds": 6300,
      "engine": "moto-club"
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

#### bike.toml Discovery

Bike commands look for `bike.toml` in the current working directory. If not found, they search up to the git root.

#### `bike.toml` format

```toml
name = "api-service"
engine = "axum"

[build]
target = "release"

[deploy]
replicas = 2
port = 8080

[health]
path = "/health"

[resources]
cpu = "500m"
memory = "512Mi"
```

**Required fields:** `name`

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
  "name": "local",
  "type": "k3s",
  "status": "ready"
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
- Garage connectivity via WireGuard (see wgtunnel.md)

## References

- [wgtunnel.md](wgtunnel.md) - WireGuard connectivity
- [garage-lifecycle.md](garage-lifecycle.md) - Garage state machine
- [bike.md](bike.md) - Bike deployment model
- [moto-club.md](moto-club.md) - Server API
