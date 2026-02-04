# Local Cluster

| | |
|--------|----------------------------------------------|
| Version | 0.2 |
| Status | Ready to Rip |
| Last Updated | 2026-02-04 |

## Overview

Local Kubernetes cluster using k3d (k3s-in-Docker). Provides the cluster where garages and bikes run.

**Scope:**
- k3d cluster creation
- Local container registry
- Status reporting

**Out of scope (future):**
- Component deployment (moto-club, keybox, postgres) - separate concern
- Remote/cloud clusters
- Multi-node clusters

## Specification

### Prerequisites

| Requirement | Why |
|-------------|-----|
| Docker or Colima | k3d runs k3s inside Docker |
| k3d | CLI tool for managing k3s clusters in Docker |

### Why k3d

| Aspect | k3d | Bare k3s |
|--------|-----|----------|
| Installation | Single binary | System service |
| Isolation | Runs in Docker | Runs on host |
| Cleanup | `k3d cluster delete` | Manual cleanup |
| Mac support | Works via Docker | Requires VM |

### Cluster Configuration

**Cluster name:** `moto`

**k3d create command:**

```bash
k3d cluster create moto \
  --api-port 6550 \
  --port "80:80@loadbalancer" \
  --port "443:443@loadbalancer" \
  --registry-create moto-registry:5000 \
  --k3s-arg "--disable=traefik@server:0"
```

| Flag | Purpose |
|------|---------|
| `--api-port 6550` | K8s API on predictable port |
| `--port 80/443` | HTTP(S) ingress |
| `--registry-create` | Local registry for images |
| `--disable=traefik` | Don't need default ingress |

**kubeconfig:** k3d merges into `~/.kube/config` with context `k3d-moto`.

### Init Flow

`moto cluster init`:

1. Check Docker is running
2. Check if cluster already exists (idempotent - return success)
3. Run `k3d cluster create moto ...`
4. Wait for cluster ready (API responds)
5. Print success message with registry info

That's it. No component deployment.

### Status Checks

`moto cluster status` reports:

```
Cluster: moto (k3d)
Status: running

  K8s API:   healthy (https://localhost:6550)
  Registry:  healthy (localhost:5000)
```

**JSON output (`--json`):**

```json
{
  "name": "moto",
  "type": "k3d",
  "status": "running",
  "api": {
    "endpoint": "https://localhost:6550",
    "healthy": true
  },
  "registry": {
    "endpoint": "localhost:5000",
    "healthy": true
  }
}
```

**Status values:**

| Status | Meaning |
|--------|---------|
| `running` | Cluster exists and API responds |
| `stopped` | Cluster exists but not running |
| `not_found` | No cluster named `moto` |

### Port Mapping

| Host Port | Service |
|-----------|---------|
| 6550 | K8s API |
| 80 | HTTP ingress |
| 443 | HTTPS ingress |
| 5000 | Container registry |

### Error Handling

| Error | Resolution |
|-------|------------|
| `Docker not running` | Start Docker/Colima |
| `Port in use` | Stop conflicting service |
| `Cluster already exists` | No-op, return success |

### CLI Reference

#### `moto cluster init`

```
Usage: moto cluster init [options]

Options:
  --force    Delete existing cluster and recreate
```

**Exit codes:** 0 success, 1 error

#### `moto cluster status`

```
Usage: moto cluster status [options]

Options:
  --json, -j    Output as JSON
```

**Exit codes:** 0 running, 1 not running or error

## Deferred

- `moto cluster stop` - Stop cluster
- `moto cluster start` - Start stopped cluster
- `moto cluster delete` - Delete cluster
- Component deployment (postgres, moto-club, keybox)

## References

- [moto-cli.md](moto-cli.md) - CLI command definitions
- [container-system.md](container-system.md) - How images are built

## Changelog

### v0.2 (2026-02-04)
- Add k3d as explicit prerequisite
- Document global flags (--json, --quiet, --verbose, --context) available on all commands
- Document `moto cluster init` JSON output format (status: "created" or "exists")

### v0.1 (2026-01-27)
- Initial specification
