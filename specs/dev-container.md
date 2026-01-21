# Dev Container

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Last Updated | 2026-01-20 |

## Overview

The dev container is the garage environment - where Claude Code wrenches on the codebase. This spec defines **what's inside** the garage container. For how it's built, see [container-system.md](container-system.md).

## Specification

### Philosophy

- **Reproducible**: Same container = same environment, always
- **Complete**: Everything Claude Code needs to build, test, run
- **Root access**: AI needs full control inside the sandbox
- **Isolated**: Security comes from the container/namespace boundary

### Included Tooling

**Languages:**

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.83.0 (pinned) | Primary language |
| Node.js | 22.x LTS (pinned) | For tooling that needs it |
| Python | 3.12 (pinned) | For scripts, tooling |

**Rust toolchain:**

| Tool | Purpose |
|------|---------|
| cargo | Build, run, test |
| rustfmt | Code formatting |
| clippy | Linting |
| rust-analyzer | IDE support |
| cargo-watch | Auto-rebuild on changes |
| cargo-nextest | Modern test runner |
| cargo-audit | Security vulnerability scanner |
| cargo-deny | License/vulnerability auditing |
| cargo-edit | Cargo.toml manipulation |
| cargo-expand | Macro debugging |
| cargo-llvm-cov | Code coverage |
| mold | Fast linker |
| sccache | Shared compilation cache |
| sqlx-cli | Database migrations |

**System libraries:**

| Library | Purpose |
|---------|---------|
| pkg-config | Build system helper |
| openssl | TLS/crypto |
| libpq | PostgreSQL client library |
| clang | C compiler (required for mold linker) |

**Version control:**

| Tool | Purpose |
|------|---------|
| git | VCS |
| jj (jujutsu) | Garage workflow - see [jj-workflow.md](jj-workflow.md) |
| gh | GitHub CLI |

**Database clients:**

| Tool | Purpose |
|------|---------|
| psql | PostgreSQL client |
| redis-cli | Redis client |

**General tools:**

| Tool | Purpose |
|------|---------|
| curl | HTTP client |
| jq | JSON processing |
| yq | YAML processing |
| ripgrep (rg) | Fast search |
| fd | Fast find |
| bat | Better cat |
| htop | Process monitoring |
| tree | Directory visualization |

**Kubernetes:**

| Tool | Purpose |
|------|---------|
| kubectl | K8s CLI |
| k9s | K8s TUI |
| helm | Package manager |

**AI:**

| Tool | Purpose |
|------|---------|
| claude-code | Claude CLI for wrenching |

**Connectivity (TODO - WireGuard):**

| Tool | Purpose |
|------|---------|
| wireguard-tools | WireGuard client for tunnel |
| openssh-server | SSH server for terminal access |

> **TODO: Add WireGuard + SSH for terminal access**
> - Garage pod runs WireGuard daemon (registers with moto-club)
> - SSH server listens on WireGuard interface
> - CLI connects via WireGuard tunnel → SSH
> - See [wgtunnel.md](wgtunnel.md) for details

### Claude Code Configuration

Claude Code connects to ai-proxy instead of direct API calls:

```bash
# Environment variables (set in container)
ANTHROPIC_BASE_URL="http://ai-proxy.moto-system.svc.cluster.local:8080"
ANTHROPIC_API_KEY="garage-${GARAGE_ID}"  # Dummy, proxy handles real key
```

The ai-proxy:
- Recognizes garage identity from the request
- Injects real API credentials
- Tracks usage per garage
- Enforces rate limits

### Network Configuration

Garage needs access to:

| Service | Endpoint | Purpose |
|---------|----------|---------|
| ai-proxy | `ai-proxy.moto-system:8080` | AI provider access |
| keybox | `keybox.moto-system:8080` | Secrets (via SPIFFE) |
| postgres | `postgres.moto-garage-{id}:5432` | Local dev database |
| redis | `redis.moto-garage-{id}:6379` | Local dev cache |
| internet | (egress allowed) | Package downloads, docs |

**Allowed egress:**
- `ai-proxy.moto-system`
- `keybox.moto-system`
- `*.moto-garage-{id}` (own namespace)
- `crates.io`, `github.com`, `npmjs.org` (package registries)

**Denied:**
- Other garage namespaces
- Production bike namespaces
- Direct cloud provider APIs
- Cloud metadata service (`169.254.169.254`) - prevents credential theft

### Volume Mounts

| Mount | Path | Type | Purpose |
|-------|------|------|---------|
| Code | `/workspace` | PVC | Repo checkout, persists across restarts |
| Cargo cache | `/root/.cargo` | PVC | Rust build cache, shared across garages |
| Target dir | `/workspace/target` | emptyDir | Build artifacts, ephemeral |

**Notes:**
- `/workspace` is a PVC so uncommitted work survives pod restarts
- Cargo cache is shared PVC to speed up builds across garages
- Target directory is ephemeral (large, regenerable)
- Nix store is baked into the image, not mounted

### Environment Variables

```bash
# System
HOME="/root"
TERM="xterm-256color"
SHELL="/bin/bash"

# Identity (injected by K8s)
GARAGE_ID="abc123"
GARAGE_NAME="feature-tokenization"
POD_NAME="moto-garage-abc123"
POD_NAMESPACE="moto-garage-abc123"

# Paths
WORKSPACE="/workspace"
CARGO_HOME="/root/.cargo"
CARGO_TARGET_DIR="/workspace/target"

# Rust
RUST_BACKTRACE="1"
RUST_LOG="info"
RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold"
RUSTC_WRAPPER="sccache"

# AI Proxy
ANTHROPIC_BASE_URL="http://ai-proxy.moto-system.svc.cluster.local:8080"

# Database (credentials from keybox, not hardcoded)
DATABASE_HOST="postgres.moto-garage-${GARAGE_ID}.svc.cluster.local"
DATABASE_PORT="5432"
DATABASE_NAME="moto"
# DATABASE_URL assembled at runtime from keybox secrets

# Redis
REDIS_URL="redis://redis.moto-garage-${GARAGE_ID}.svc.cluster.local:6379"

# Keybox
KEYBOX_URL="http://keybox.moto-system.svc.cluster.local:8080"

# SPIFFE (for keybox auth)
SPIFFE_ENDPOINT_SOCKET="unix:///run/spire/sockets/agent.sock"

# Telemetry
DO_NOT_TRACK="1"

# TLS
SSL_CERT_FILE="/etc/ssl/certs/ca-bundle.crt"
```

### Security Model

**Philosophy: The container IS the sandbox.**

Inside the garage, Claude Code has full control. Isolation comes from the container and namespace boundary.

**Inside garage (unrestricted):**
- Root access (can install packages, modify anything)
- Full filesystem access
- Can run any commands
- This is intentional - AI needs freedom to wrench

**Isolation (at the boundary):**
- K8s namespace isolation (each garage is its own namespace)
- NetworkPolicy controls egress
- Resource quotas prevent runaway usage
- TTL ensures cleanup

**Secrets:**
- No secrets baked into image
- All credentials fetched from keybox at runtime
- SPIFFE identity for authentication
- AI can't access production secrets (scoped to garage)

**What garage CAN'T do:**
- Access other garages
- Access production bikes
- Escape the container (no privileged mode, no host mounts)
- Exceed resource limits
- Access cloud metadata service
- Create/modify K8s resources outside own namespace

**Tool restrictions:**
- `kubectl`/`helm`: RBAC limited to read-only access to own namespace
- `gh` CLI: Token scoped to repo read/write only (no org admin, no other repos)
- `git`/`jj`: Auth via scoped deploy key or token (not user credentials)

**Container security context:**
```yaml
securityContext:
  allowPrivilegeEscalation: false
  seccompProfile:
    type: RuntimeDefault
  # Note: runs as root inside, but constrained by namespace/network
```

### Resource Limits

Default limits per garage:

| Resource | Request | Limit |
|----------|---------|-------|
| CPU | 2 cores | 4 cores |
| Memory | 4Gi | 8Gi |
| Ephemeral storage | 10Gi | 20Gi |

**Rationale:**
- Rust compilation is CPU and memory intensive
- 8Gi memory prevents OOM during `cargo build`
- 4 cores allows parallel compilation
- 20Gi storage for cargo cache, target directory

Can be overridden per-garage:
```bash
moto garage open --cpu 8 --memory 16Gi
```

## References

- [container-system.md](container-system.md) - How the container is built
- [garage-lifecycle.md](garage-lifecycle.md) - How garages are managed
- [garage-isolation.md](garage-isolation.md) - Network policies, quotas
- [keybox.md](keybox.md) - How secrets are fetched
- [ai-proxy.md](ai-proxy.md) - How Claude Code connects to AI
