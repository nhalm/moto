# Dev Container

| | |
|--------|----------------------------------------------|
| Version | 0.7 |
| Last Updated | 2026-01-24 |

## Overview

The dev container is the garage environment - where Claude Code wrenches on the codebase. This is a **NixOS container** that provides a fully reproducible, declarative development environment.

**Key architecture decisions:**
- **NixOS** as the container OS (not just Nix-on-Linux)
- **Flakes** for reproducible builds and dependency management
- **Root flake** at repo root (`moto/flake.nix`) provides devShell
- **Container config** in `moto/infra/pkgs/` and `moto/infra/modules/`

## Specification

### Philosophy

- **Reproducible**: NixOS = same system configuration every time
- **Declarative**: Entire OS defined in Nix, version-controlled
- **Complete**: Everything Claude Code needs to build, test, run
- **Root access**: AI needs full control inside the sandbox
- **Isolated**: Security comes from the container/namespace boundary

### Why NixOS (not just Nix)

| Approach | Description | Trade-off |
|----------|-------------|-----------|
| Nix on Alpine/Ubuntu | Nix package manager on top of another OS | Mixing package managers, less reproducible |
| **NixOS container** | Entire OS is NixOS, everything from Nix | Fully declarative, larger image, steeper learning curve |

We use NixOS because:
- Everything (packages, services, users) is declared in Nix
- Services (SSH, WireGuard daemon) are easy to configure
- System upgrades are atomic and rollback-able
- No mixing of package managers

### Project Structure

```
moto/
├── flake.nix                    # Root flake - single source of truth
├── flake.lock                   # Pinned dependencies
└── infra/
    ├── pkgs/                    # Package/container definitions
    │   ├── moto-garage.nix         # This container's definition
    │   ├── moto-engine.nix      # Engine containers (future)
    │   └── default.nix          # Exports all packages
    ├── modules/                 # Reusable NixOS modules
    │   ├── base.nix             # Common system settings
    │   ├── ssh.nix              # SSH server configuration
    │   ├── dev-tools.nix        # Development tooling
    │   └── wireguard.nix        # WireGuard support
    └── smoke-test.sh            # Container smoke tests
```

**Root flake (`moto/flake.nix`):**
- Provides `devShells.default` with all development tools
- Provides `packages.moto-garage` (this container)
- Provides `packages.moto-engine-*` (runtime containers)
- Imports definitions from `./infra/pkgs/`

**Container definition (`moto/infra/pkgs/moto-garage.nix`):**
- Uses `dockerTools.buildLayeredImage`
- Imports modules from `../modules/`
- Configures services (SSH, WireGuard daemon)

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

### NixOS Services

The container runs these NixOS services:

**SSH Server:**
```nix
services.openssh = {
  enable = true;
  settings = {
    PermitRootLogin = "yes";
    PasswordAuthentication = false;
  };
};
```

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

Container builds are defined in the root `flake.nix`, which imports from `infra/pkgs/`. See [container-system.md](container-system.md) for the complete build pipeline.

**Build commands:**

```bash
# Build the garage (dev) container from repo root
nix build .#moto-garage
docker load < result
# Image is now available as moto-garage:latest

# Or use Makefile targets
make docker-build-moto-garage
```

**Registry (when needed):**
- Local: `localhost:5000/moto-garage:latest`
- Remote: `ghcr.io/<org>/moto-garage:latest`

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

### Example Root Flake

```nix
# moto/flake.nix
{
  description = "Moto - fintech infrastructure";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rustToolchain = pkgs.rust-bin.stable."1.85.0".default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust
            rustToolchain
            cargo-watch
            cargo-nextest
            cargo-audit
            cargo-deny
            cargo-edit
            cargo-expand
            mold
            sccache
            sqlx-cli

            # Build deps
            pkg-config
            openssl
            postgresql.lib
            clang

            # Version control
            git
            jujutsu
            gh

            # Database clients
            postgresql
            redis

            # General tools
            curl
            jq
            yq
            ripgrep
            fd
            bat
            htop
            tree

            # Kubernetes
            kubectl
            k9s
            kubernetes-helm

            # AI - claude-code installed via shell script, not nix
            # See: curl -fsSL https://claude.ai/install.sh | bash

            # Connectivity
            wireguard-tools
            openssh
          ];

          shellHook = ''
            export RUST_BACKTRACE=1
            export RUSTFLAGS="-C linker=clang -C link-arg=-fuse-ld=mold"
          '';
        };
      }
    );
}
```

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
