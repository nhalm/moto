# Dev Container

| | |
|--------|----------------------------------------------|
| Version | 0.9 |
| Status | Ripping |
| Last Updated | 2026-01-26 |

## Overview

The dev container is the garage environment - where Claude Code wrenches on the codebase. This is a **Nix-built container** using `dockerTools.buildLayeredImage` for fully reproducible, declarative builds.

**Key architecture decisions:**
- **Nix dockerTools** for container builds (reproducible, layered images)
- **Modular Nix config** in `infra/modules/` for composable container definitions
- **Root flake** at repo root (`moto/flake.nix`) exports container packages
- **Multi-arch** via flake outputs (`x86_64-linux`, `aarch64-linux`)

## Specification

### Philosophy

- **Reproducible**: NixOS = same system configuration every time
- **Declarative**: Entire OS defined in Nix, version-controlled
- **Complete**: Everything Claude Code needs to build, test, run
- **Root access**: AI needs full control inside the sandbox
- **Isolated**: Security comes from the container/namespace boundary

### Why Nix dockerTools

| Approach | Description | Trade-off |
|----------|-------------|-----------|
| Pure Dockerfile | Install packages via apt/apk | Simple but not reproducible |
| Dockerfile + Nix | Dockerfile shell, Nix inside | Mixed build systems |
| **Nix dockerTools** | Build image entirely with Nix | Fully reproducible, layered, content-addressed |

We use Nix dockerTools because:
- **Reproducible**: Same inputs always produce identical images (content-addressed)
- **Layered**: `buildLayeredImage` creates efficient Docker layers automatically
- **Modular**: Compose container contents from reusable Nix modules
- **Multi-arch**: Flake outputs for both `x86_64-linux` and `aarch64-linux`

### Project Structure

```
moto/
├── flake.nix                    # Root flake - exports packages and devShell
├── flake.lock                   # Pinned dependencies
└── infra/
    ├── pkgs/                    # Container package definitions
    │   ├── moto-garage.nix      # Garage container definition
    │   └── default.nix          # Exports all packages
    ├── modules/                 # Reusable Nix modules
    │   ├── base.nix             # Core packages (bash, coreutils, cacert)
    │   ├── dev-tools.nix        # Rust toolchain, build tools, K8s tools
    │   ├── ssh.nix              # OpenSSH for terminal access
    │   └── wireguard.nix        # WireGuard for tunnel connectivity
    └── smoke-test.sh            # Container smoke tests
```

**Root flake (`moto/flake.nix`):**
- Provides `devShells.default` with all development tools
- Provides `packages.moto-garage` for Linux systems
- Imports container definitions from `./infra/pkgs/`

**Container definition (`moto/infra/pkgs/moto-garage.nix`):**
- Uses `dockerTools.buildLayeredImage` for efficient layering
- Composes modules: base + ssh + dev-tools + wireguard
- Configures working directory, environment variables, volumes

### Included Tooling

All tools are installed via Nix in the devShell/container.

**Languages:**

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.85 stable | Primary language |
| Node.js | 22.x LTS | For tooling (claude-code) |

**Rust toolchain:**

| Tool | Nix Package | Purpose |
|------|-------------|---------|
| cargo | (bundled with rust) | Build, run, test |
| rustfmt | (bundled with rust) | Code formatting |
| clippy | (bundled with rust) | Linting |
| rust-analyzer | `rust-analyzer` | IDE support |
| cargo-watch | `cargo-watch` | Auto-rebuild on changes |
| cargo-nextest | `cargo-nextest` | Modern test runner |
| cargo-audit | `cargo-audit` | Security vulnerability scanner |
| cargo-deny | `cargo-deny` | License/vulnerability auditing |
| cargo-edit | `cargo-edit` | Cargo.toml manipulation |
| cargo-expand | `cargo-expand` | Macro debugging |
| mold | `mold` | Fast linker |
| sccache | `sccache` | Shared compilation cache |
| sqlx-cli | `sqlx-cli` | Database migrations |

**System libraries:**

| Library | Nix Package | Purpose |
|---------|-------------|---------|
| pkg-config | `pkg-config` | Build system helper |
| openssl | `openssl` | TLS/crypto |
| libpq | `postgresql.lib` | PostgreSQL client library |
| clang | `clang` | C compiler (for mold linker) |

**Version control:**

| Tool | Nix Package | Purpose |
|------|-------------|---------|
| git | `git` | VCS |
| jj (jujutsu) | `jujutsu` | Garage workflow - see [jj-workflow.md](jj-workflow.md) |
| gh | `gh` | GitHub CLI |

**Database clients:**

| Tool | Nix Package | Purpose |
|------|-------------|---------|
| psql | `postgresql` | PostgreSQL client |
| redis-cli | `redis` | Redis client |

**General tools:**

| Tool | Nix Package | Purpose |
|------|-------------|---------|
| curl | `curl` | HTTP client |
| jq | `jq` | JSON processing |
| yq | `yq` | YAML processing |
| ripgrep | `ripgrep` | Fast search |
| fd | `fd` | Fast find |
| bat | `bat` | Better cat |
| htop | `htop` | Process monitoring |
| tree | `tree` | Directory visualization |

**Kubernetes:**

| Tool | Nix Package | Purpose |
|------|-------------|---------|
| kubectl | `kubectl` | K8s CLI |
| k9s | `k9s` | K8s TUI |
| helm | `kubernetes-helm` | Package manager |

**AI:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| claude-code | Native binary (shell script) | Claude CLI for wrenching |

Claude Code is installed via the official shell script, not nixpkgs:
```bash
curl -fsSL https://claude.ai/install.sh | bash
```
This is run during container build. The binary installs to `~/.local/bin/claude`.

**Connectivity:**

| Tool | Nix Package | Purpose |
|------|-------------|---------|
| wireguard-tools | `wireguard-tools` | WireGuard client for tunnel |
| openssh | `openssh` | SSH server for terminal access |

### Claude Code Configuration

**v1 (Local Dev):** User provides API key directly.

```bash
# User sets this when creating garage or in their environment
ANTHROPIC_API_KEY="sk-ant-..."
```

The API key can be:
- Passed as env var when starting the garage
- Stored in user's local config (not in container image)

**Future (with ai-proxy):** Claude Code connects via ai-proxy:

```bash
ANTHROPIC_BASE_URL="http://ai-proxy.moto-system.svc.cluster.local:8080"
ANTHROPIC_API_KEY="garage-${GARAGE_ID}"  # Proxy handles real key
```

### Services

The container includes these services (configured in Dockerfile):

**SSH Server:**
- OpenSSH installed via Nix
- Configured at container startup if needed
- Root login enabled for AI access

**WireGuard (configured by moto-garage-wgtunnel daemon):**
- Daemon registers with moto-club on startup
- Configures WireGuard interface dynamically
- See [moto-wgtunnel.md](moto-wgtunnel.md) for details

### Network Configuration

Garage needs access to:

| Service | Endpoint | Purpose |
|---------|----------|---------|
| Anthropic API | `api.anthropic.com` | Claude Code (v1, direct) |
| keybox | `keybox.moto-system:8080` | Secrets (future) |
| postgres | `postgres.moto-garage-{id}:5432` | Local dev database |
| redis | `redis.moto-garage-{id}:6379` | Local dev cache |
| internet | (egress allowed) | Package downloads, docs |

**Allowed egress:**
- `api.anthropic.com` (Claude API, v1)
- `*.moto-garage-{id}` (own namespace)
- `crates.io`, `github.com`, `npmjs.org` (package registries)
- `cache.nixos.org` (Nix binary cache)

**Denied:**
- Other garage namespaces
- Production bike namespaces
- Cloud metadata service (`169.254.169.254`) - prevents credential theft

### Volume Mounts

| Mount | Path | Type | Purpose |
|-------|------|------|---------|
| Code | `/workspace` | PVC | Repo checkout, persists across restarts |
| Cargo cache | `/root/.cargo` | PVC | Rust build cache, shared across garages |
| Target dir | `/workspace/target` | emptyDir | Build artifacts, ephemeral |
| Nix store | `/nix` | PVC | Nix store, shared across garages |

**Notes:**
- `/workspace` is a PVC so uncommitted work survives pod restarts
- Cargo cache is shared PVC to speed up builds across garages
- Target directory is ephemeral (large, regenerable)
- Nix store is shared to avoid re-downloading packages

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

# AI (v1 - user provides key)
# ANTHROPIC_API_KEY set by user

# Database (credentials from env or keybox)
DATABASE_HOST="postgres.moto-garage-${GARAGE_ID}.svc.cluster.local"
DATABASE_PORT="5432"
DATABASE_NAME="moto"

# Redis
REDIS_URL="redis://redis.moto-garage-${GARAGE_ID}.svc.cluster.local:6379"

# Nix
NIX_PATH="nixpkgs=flake:nixpkgs"

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
- Can use `nix-shell`, `nix build`, etc.
- This is intentional - AI needs freedom to wrench

**Isolation (at the boundary):**
- K8s namespace isolation (each garage is its own namespace)
- NetworkPolicy controls egress
- Resource quotas prevent runaway usage
- TTL ensures cleanup

**Secrets:**
- No secrets baked into image
- API keys passed as env vars (v1) or fetched from keybox (future)
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

### Building the Container

Container builds use Nix flake outputs. The Makefile auto-detects architecture.

**Build commands:**

```bash
# Build via Makefile (auto-detects architecture)
make docker-build-moto-garage

# Or directly with Nix (specify architecture)
nix build .#packages.aarch64-linux.moto-garage  # ARM (Apple Silicon)
nix build .#packages.x86_64-linux.moto-garage   # Intel/AMD
docker load < result
```

**Registry:**
- Local: `moto-garage:latest`
- Remote: `ghcr.io/<org>/moto-garage:latest`

**Architecture notes:**
- Mac (ARM): Builds `aarch64-linux` via flake output
- Mac (Intel): Builds `x86_64-linux` via flake output
- CI: Can build both architectures
- k3s on Mac: Runs matching architecture natively

### Testing the Container

Smoke tests verify the container builds correctly and contains expected tooling.

**Makefile targets:**

| Target | Purpose |
|--------|---------|
| `docker-build-moto-garage` | Build moto-garage image |
| `docker-test-moto-garage` | Build + run smoke tests |
| `docker-shell-moto-garage` | Interactive shell for debugging |

**Smoke tests verify:**

| Category | Checks |
|----------|--------|
| Core tools | rustc, cargo, git, jj, kubectl present and executable |
| Environment | RUST_BACKTRACE, CARGO_HOME, WORKSPACE set correctly |
| Rust compilation | Can compile and run a simple Rust program |

**Test script:** `infra/smoke-test.sh`

**Usage:**

```bash
make docker-test-moto-garage

# Or directly
./infra/smoke-test.sh

# Keep container for debugging
./infra/smoke-test.sh --keep
```

### Example Container Definition

```nix
# infra/pkgs/moto-garage.nix
{ pkgs, rustToolchain }:

let
  base = import ../modules/base.nix { inherit pkgs; };
  ssh = import ../modules/ssh.nix { inherit pkgs; };
  devTools = import ../modules/dev-tools.nix { inherit pkgs rustToolchain; };
  wireguard = import ../modules/wireguard.nix { inherit pkgs; };

  allContents = base.contents ++ ssh.contents ++ devTools.contents ++ wireguard.contents;
  allEnv = base.env ++ devTools.env;
in
pkgs.dockerTools.buildLayeredImage {
  name = "moto-garage";
  tag = "latest";
  contents = allContents;
  config = {
    Cmd = [ "/bin/bash" ];
    WorkingDir = "/workspace";
    Env = allEnv;
    Volumes = {
      "/workspace" = {};
      "/root/.cargo" = {};
    };
  };
}
```

The modular design allows reusing components across different container types.

## Deferred Items

### ai-proxy Integration (Future)

When ai-proxy is implemented:
- Claude Code connects via proxy instead of direct API
- Proxy handles credentials, rate limiting, usage tracking
- See [ai-proxy.md](ai-proxy.md)

### Keybox Integration (Future)

When keybox is implemented:
- Secrets fetched at runtime via SPIFFE identity
- No API keys in env vars
- See [keybox.md](keybox.md)

### SPIFFE Identity (Future)

Each garage gets a SPIFFE identity:
```
spiffe://moto.local/garage/{garage-id}
```

## Changelog

### v0.9 (2026-01-26)
- Correct spec to match implementation: Nix dockerTools approach
- Document modular structure: base, dev-tools, ssh, wireguard modules
- Update build commands to show `nix build` workflow
- Multi-arch via flake outputs, not docker buildx
- Mark as Ripping (implementation complete)

### v0.8 (2026-01-25)
- (Spec update that diverged from implementation - corrected in v0.9)

### v0.7 (2026-01-24)
- Reorganize infra directory: `pkgs/`, `modules/`, `machines/` structure
- Rename from `moto-dev` to `moto-garage` for metaphor consistency
- Container definition moves to `infra/pkgs/moto-garage.nix`
- Build command: `nix build .#moto-garage`
- Smoke test path: `infra/smoke-test.sh`
- Update smoke test path to `infra/smoke-test.sh`

### v0.6 (2026-01-23)
- Add "Testing the Container" section with smoke test specification
- Update "Building the Container" to reference root flake.nix

### v0.5 (2026-01-23)
- Claude Code: Install via native binary shell script, not nixpkgs

### v0.4 (2026-01-23)
- NixOS as container base (not just Nix-on-Linux)
- Rust 1.85 (synced with Cargo.toml)
- Root flake at `moto/flake.nix`, container config in `infra/dev-container/`
- Claude Code from nixpkgs, user provides API key for v1
- SSH server and WireGuard tools included
- Example flake provided

### v0.3 (2026-01-20)
- Initial tooling specification
- Volume mounts, environment variables, security model

## References

- [container-system.md](container-system.md) - Container build pipeline
- [garage-lifecycle.md](garage-lifecycle.md) - How garages are managed
- [garage-isolation.md](garage-isolation.md) - Network policies, quotas
- [moto-wgtunnel.md](moto-wgtunnel.md) - WireGuard tunnel system
- [moto-club.md](moto-club.md) - Garage orchestration
