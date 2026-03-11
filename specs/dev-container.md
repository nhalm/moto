# Dev Container

| | |
|--------|----------------------------------------------|
| Version | 0.19 |
| Status | Ripping |
| Last Updated | 2026-03-05 |

## Overview

The dev container is the garage environment - where Claude Code wrenches on the codebase. This is a **Nix-built container** using `dockerTools.buildLayeredImage` for fully reproducible, declarative builds.

**Key architecture decisions:**
- **Nix dockerTools** for container builds (reproducible, layered images)
- **Modular flake** using flake-parts for clean composition
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

Modular structure with packages and modules:

```
moto/
├── flake.nix                    # Root flake - devShells + imports infra/pkgs
├── flake.lock                   # Pinned dependencies
└── infra/
    ├── pkgs/                    # Container package definitions
    │   ├── default.nix          # Exports all packages
    │   └── moto-garage.nix      # Garage container definition
    ├── modules/                 # Reusable module components
    │   ├── base.nix             # Core system tools (bash, coreutils, etc.)
    │   ├── dev-tools.nix        # Development tooling (Rust, cargo, etc.)
    │   ├── terminal.nix         # Terminal daemon (ttyd + tmux)
    │   └── wireguard.nix        # WireGuard tools
    └── smoke-test.sh            # Container smoke tests
```

**Why this structure:**
- Modular: each module returns `{ contents, env }` for composition
- `buildEnv` wraps contents to avoid file collisions
- Simple direct imports, no framework dependencies
- Reusable modules across different container types

**Root flake (`moto/flake.nix`):**
- Defines `devShells.default` with development tools
- Imports container packages from `./infra/pkgs`
- Uses `eachDefaultSystem` for multi-platform support

**Container definition (`moto/infra/pkgs/moto-garage.nix`):**
- Uses `dockerTools.buildLayeredImage` for efficient layered images
- Wraps contents with `buildEnv` (handles file collisions)
- Imports modules from `../modules/` and combines them

### Included Tooling

All tools are installed via Nix in the devShell/container.

**Languages:**

| Tool | Version | Purpose |
|------|---------|---------|
| Rust | 1.88 stable | Primary language |
| Node.js | 22.x LTS | For tooling (claude-code) |

**Rust toolchain:**

The Rust toolchain uses `rust-bin.stable."X.Y".minimal` (not `.default`) to avoid pulling in rust-docs (~700MB). Required components are added via `extensions`.

| Tool | Nix Package | Purpose |
|------|-------------|---------|
| cargo | (bundled with rust) | Build, run, test |
| rustfmt | extension: `rustfmt` | Code formatting |
| clippy | extension: `clippy` | Linting |
| rust-analyzer | extension: `rust-analyzer` | IDE support |
| rust-src | extension: `rust-src` | Source for IDE navigation |
| cargo-watch | `cargo-watch` | Auto-rebuild on changes |
| cargo-nextest | `cargo-nextest` | Modern test runner |
| mold | `mold` | Fast linker |
| sccache | `sccache` | Shared compilation cache |
| sqlx-cli | `sqlx-cli` | Database migrations |

**System libraries:**

| Library | Nix Package | Purpose |
|---------|-------------|---------|
| pkg-config | `pkg-config` | Build system helper |
| openssl | `openssl` | TLS/crypto |
| libpq | `postgresql.lib` | PostgreSQL client library |

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
| ttyd | `ttyd` | WebSocket terminal daemon |
| tmux | `tmux` | Terminal multiplexer for session persistence |

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

**Terminal Daemon (ttyd + tmux):**
- ttyd listens on port 7681 (WebSocket)
- Spawns tmux for session persistence
- Working directory: `/workspace/<repo-name>/` (set after repo clone)
- Runs as: root
- Process management: shell entrypoint (`exec ttyd`), K8s pod restart policy handles restarts
- Health check: TCP probe on port 7681
- No authentication required (WireGuard tunnel is auth boundary)
- See [moto-wgtunnel.md](moto-wgtunnel.md) for connection details

**Session persistence:**
- First connect → creates tmux session, attaches
- Disconnect → tmux session keeps running (processes survive)
- Reconnect → reattaches to existing tmux session
- Multiple clients → all attach to same tmux session (mirrored view)

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
| workspace | `/workspace` | PersistentVolumeClaim | Repo checkout, persists across restarts |
| tmp | `/tmp` | emptyDir | Temporary files, ephemeral |
| var-tmp | `/var/tmp` | emptyDir | Temporary files, ephemeral |
| home | `/root` | emptyDir | Home directory, ephemeral |
| cargo | `/root/.cargo` | emptyDir | Rust build cache, ephemeral |
| var-lib-apt | `/var/lib/apt` | emptyDir | apt package state, ephemeral |
| var-cache-apt | `/var/cache/apt` | emptyDir | apt package cache, ephemeral |
| usr-local | `/usr/local` | emptyDir | Locally installed tools, ephemeral |
| wireguard-config | `/etc/wireguard` | ConfigMap | WireGuard config pushed by moto-club |
| wireguard-keys | `/run/wireguard` | Secret | WireGuard private/public keys |
| garage-svid | `/var/run/secrets/svid` | Secret | SPIFFE SVID for keybox auth |

**Notes:**
- `/workspace` is a PVC so uncommitted work survives pod restarts
- Target directory is ephemeral (large, regenerable)
- `/nix` is NOT mounted as a volume — the image provides `/nix/store` with all tools pre-installed (read-only via `readOnlyRootFilesystem`). Mounting an emptyDir over `/nix` would shadow the image contents and break all symlinks.

### Environment Variables

```bash
# System
HOME="/root"
TERM="xterm-256color"
SHELL="/bin/bash"

# Identity (injected by K8s)
MOTO_GARAGE_BRANCH="feature-tokenization"
MOTO_GARAGE_NAMESPACE="moto-garage-abc123"

# Paths
WORKSPACE="/workspace"
CARGO_HOME="/root/.cargo"
CARGO_TARGET_DIR="/workspace/target"

# Rust
RUST_BACKTRACE="1"
RUST_LOG="info"
RUSTFLAGS="-C link-arg=-fuse-ld=mold"
RUSTC_WRAPPER="sccache"

# AI (v1 - user provides key)
# ANTHROPIC_API_KEY set by user

# Database (injected when --with-postgres is used)
POSTGRES_HOST="postgres.moto-garage-abc123.svc.cluster.local"
POSTGRES_PORT="5432"
POSTGRES_DB="dev"
POSTGRES_USER="dev"
POSTGRES_PASSWORD="<from secret>"
DATABASE_URL="postgresql://dev:<password>@postgres:5432/dev"

# Redis (injected when --with-redis is used)
REDIS_HOST="redis.moto-garage-abc123.svc.cluster.local"
REDIS_PORT="6379"
REDIS_PASSWORD="<from secret>"
REDIS_URL="redis://:password@redis:6379"

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
- `kubectl`/`helm`: No K8s API access from inside the container — `automountServiceAccountToken: false` means no service account token is mounted
- `gh` CLI: Token scoped to repo read/write only (no org admin, no other repos)
- `git`/`jj`: Auth via scoped deploy key or token (not user credentials)

**Container security context:**
```yaml
securityContext:
  runAsUser: 0
  runAsGroup: 0
  allowPrivilegeEscalation: false
  readOnlyRootFilesystem: true
  seccompProfile:
    type: RuntimeDefault
  capabilities:
    drop:
      - ALL
    add:
      - CHOWN
      - DAC_OVERRIDE
      - FOWNER
      - SETGID
      - SETUID
      - NET_BIND_SERVICE
  # Note: runs as root inside, but constrained by namespace/network
  # readOnlyRootFilesystem requires writable emptyDir mounts (see Volume Mounts)
```

### Resource Limits

Default limits per garage:

| Resource | Request | Limit |
|----------|---------|-------|
| CPU | 100m | 3 |
| Memory | 256Mi | 7Gi |

**Rationale:**
- 7Gi memory prevents OOM during `cargo build` while leaving headroom for supporting services
- 3 CPU cores allows parallel compilation
- Low requests (100m CPU, 256Mi memory) allow efficient scheduling on shared nodes
- See [garage-isolation.md](garage-isolation.md) for the authoritative resource limits and quota definitions

### Building the Container

Local builds use Docker-wrapped Nix: runs `nix build` inside a `nixos/nix` container. This works on Mac without a Linux builder. Architecture is auto-detected.

```bash
make build-garage    # Build the container
make test-garage     # Build + run smoke tests
make shell-garage    # Interactive shell for debugging
make push-garage     # Push to local registry
```

See [container-system.md](container-system.md) for details on the build approach.

### Build Verification (Required)

**After modifying any container-related files, you MUST build and test:**

```bash
make build-garage && make test-garage
```

**Both commands must succeed.** Container builds can fail in non-obvious ways (file collisions, missing packages, broken paths) that only surface when actually built.

**Files requiring build verification:**
- `infra/pkgs/*.nix` - Container package definitions
- `infra/modules/*.nix` - Container modules
- `flake.nix` - Nix flake (container outputs)

**Common build failures:**
| Error | Cause | Fix |
|-------|-------|-----|
| `File exists` | Package collision in `contents` | Use `buildEnv` to wrap contents |
| `are the same file` | Duplicate file copy | Remove redundant copy in `extraCommands` |
| `command not found` | Missing from PATH | Check `PATH` in env, verify package in contents |

### Testing the Container

Smoke tests verify the container builds correctly and contains expected tooling.

**Smoke tests verify:**

| Category | Checks |
|----------|--------|
| Core tools | rustc, cargo, git, jj, kubectl present and executable |
| Environment | RUST_BACKTRACE, CARGO_HOME, WORKSPACE set correctly |
| Rust compilation | Can compile and run a simple Rust program |

**Test script:** `infra/smoke-test.sh`

```bash
make test-garage

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
  terminal = import ../modules/terminal.nix { inherit pkgs; };
  devTools = import ../modules/dev-tools.nix { inherit pkgs rustToolchain; };
  wireguard = import ../modules/wireguard.nix { inherit pkgs; };

  allContents = base.contents ++ terminal.contents ++ devTools.contents ++ wireguard.contents;
  allEnv = base.env ++ devTools.env;
in
pkgs.dockerTools.buildLayeredImage {
  name = "moto-garage";
  tag = "latest";
  contents = allContents;
  config = {
    Cmd = [ "garage-entrypoint" ];
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

### v0.19 (2026-03-05)
- Fix: Container example `Cmd` from `["/bin/bash"]` to `["garage-entrypoint"]` (changed in v0.14 changelog but example was never updated)

### v0.18 (2026-03-05)
- Fix: Bump Rust version from 1.85 to 1.88 to match container-system.md v1.3 and flake.nix

### v0.17 (2026-02-24)
- Docs: Fix resource limits table to match garage-isolation.md (100m/3 CPU, 256Mi/7Gi memory)
- Docs: Update volume mounts table to list all 11 actual mounts (was only 3)
- Docs: Remove systemd reference — container uses shell entrypoint (`exec ttyd`) with K8s restart policy
- Docs: Fix K8s-injected env var names to match supporting-services.md (POSTGRES_* prefix); use MOTO_GARAGE_BRANCH and MOTO_GARAGE_NAMESPACE for garage identity
- Docs: Update security context to match garage-isolation.md (readOnlyRootFilesystem, capabilities)
- Docs: Update tool restrictions — kubectl has no K8s API access (automountServiceAccountToken=false)

### v0.16 (2026-02-22)
- Reduce image size: switch Rust toolchain from `.default` to `.minimal` profile, exclude rust-docs (~700MB savings)
- Reduce image size: drop `clang` from container, use default `cc` linker with mold (~1.4GB savings). RUSTFLAGS changes from `-C linker=clang -C link-arg=-fuse-ld=mold` to `-C link-arg=-fuse-ld=mold`
- Fix: remove `/nix` emptyDir volume mount — it shadowed the image's `/nix/store`, breaking all tool symlinks and causing `garage-entrypoint: not found` at pod startup
- Clarify: `/nix` is provided by the image (read-only), not mounted as a volume

### v0.15 (2026-02-21)
- Reduce image size: remove cargo-audit, cargo-deny, cargo-edit, cargo-expand (CI tools, not needed in dev container)
- Reduce image size: remove k9s and helm (kubectl is sufficient, k9s/helm can be installed on demand)
- Reduce image size: remove redis package (redis-cli available via supporting service container if needed)

### v0.14 (2026-02-04)
- Clarify: Claude Code is installed at runtime via install script, not at container build time
- Clarify: Container Cmd is `garage-entrypoint` (starts ttyd), not `/bin/bash`
- Clarify: K8s env vars (GARAGE_ID, DATABASE_URL, etc.) are injected by K8s, not set in image

### v0.13 (2026-02-02)
- Replace SSH with ttyd + tmux for terminal access
- Update module: `ssh.nix` → `terminal.nix`
- Update connectivity tools: remove openssh, add ttyd + tmux
- Add terminal daemon details (port 7681, systemd, health check)
- WireGuard tunnel is the sole auth boundary (no SSH keys needed)

### v0.12 (2026-01-26)
- Add "Build Verification (Required)" section with mandatory build testing
- Document common build failures and fixes
- Correct project structure to match implementation (infra/pkgs/ + infra/modules/)
- Use `buildLayeredImage` with `buildEnv` wrapper for collision-free builds

### v0.11 (2026-01-26)
- Use `buildEnv` for package composition (avoids file collisions)
- Modular structure with infra/pkgs/ and infra/modules/

### v0.10 (2026-01-26)
- Update build targets: `build-garage`, `test-garage`, `shell-garage`, `push-garage`
- Docker-wrapped Nix approach for Mac compatibility
- Simplify build section, reference container-system.md for details

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
