# Dev Container

| | |
|--------|----------------------------------------------|
| Version | 0.20 |
| Status | Ripping |
| Last Updated | 2026-04-10 |

## Overview

The dev container is the garage environment - where Claude Code wrenches on the codebase. This is a **Dockerfile-built container** using Wolfi (Chainguard) as the base image for minimal CVE footprint.

**Key architecture decisions:**
- **Wolfi base image** for minimal security vulnerabilities
- **Standard Dockerfile** for universal compatibility
- **Single-stage build** with full dev toolchain
- **Multi-arch** via Docker buildx (`linux/amd64`, `linux/arm64`)

## Specification

### Philosophy

- **Minimal CVE footprint**: Wolfi packages rebuilt daily by Chainguard
- **Standard tooling**: Universal Dockerfiles, no custom build systems
- **Complete**: Everything Claude Code needs to build, test, run
- **Root access**: AI needs full control inside the sandbox
- **Isolated**: Security comes from the container/namespace boundary

### Why Wolfi (Chainguard)

| Approach | Description | Trade-off |
|----------|-------------|-----------|
| Debian/Ubuntu | Large base with many packages | Large attack surface, slower CVE patches |
| Alpine (musl) | Small base image | musl compatibility issues with Rust/openssl/libpq |
| **Wolfi (glibc)** | Minimal base, daily rebuilds | Small footprint, glibc compatibility, fresh CVE fixes |

We use Wolfi because:
- **Minimal CVE footprint**: Packages rebuilt daily, fast security updates
- **glibc-based**: No musl compatibility issues with Rust toolchain or PostgreSQL client libs
- **Small base**: ~15MB base image
- **apk package manager**: Simple, familiar Alpine-style package management
- **Multi-arch**: Supports both `linux/amd64` and `linux/arm64`

### Project Structure

Standard Docker build structure:

```
moto/
├── rust-toolchain.toml          # Pins Rust version
├── .cargo/config.toml           # Cargo settings
└── infra/
    ├── docker/
    │   ├── Dockerfile.garage    # Garage container definition
    │   ├── Dockerfile.bike      # Bike base image
    │   ├── Dockerfile.club      # Club engine image
    │   └── Dockerfile.keybox    # Keybox engine image
    └── smoke-test.sh            # Container smoke tests
```

**Why this structure:**
- Standard: Universal Dockerfile format, no custom build systems
- Simple: Direct package installation via apk, no module composition needed
- Familiar: Any developer or AI agent can understand and modify
- Efficient: Docker layer caching handles incremental builds

**Container definition (`infra/docker/Dockerfile.garage`):**
- Single-stage build starting from `cgr.dev/chainguard/wolfi-base`
- Installs all dev tooling via `apk add` (most tools) and release binaries (jujutsu, ttyd, yq)
- Sets environment variables and working directory
- Defines entrypoint as `garage-entrypoint`

### Included Tooling

All tools are installed via apk (Wolfi packages) or release binaries.

**Languages:**

| Tool | Version | Installation |
|------|---------|--------------|
| Rust | 1.88 stable | apk (rust, cargo) |
| Node.js | 22.x LTS | apk (nodejs) |

**Rust toolchain:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| cargo | apk (bundled with rust) | Build, run, test |
| rustfmt | apk (bundled with rust) | Code formatting |
| clippy | apk (bundled with rust) | Linting |
| rust-analyzer | apk (rust-analyzer) | IDE support |
| cargo-watch | cargo install (at build time) | Auto-rebuild on changes |
| cargo-nextest | cargo install (at build time) | Modern test runner |
| mold | apk (mold) | Fast linker |
| sccache | apk (sccache) | Shared compilation cache |
| sqlx-cli | cargo install (at build time) | Database migrations |

**System libraries:**

| Library | Installation | Purpose |
|---------|--------------|---------|
| pkg-config | apk (pkgconf) | Build system helper |
| openssl | apk (openssl-dev) | TLS/crypto |
| libpq | apk (postgresql-dev) | PostgreSQL client library |

**Version control:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| git | apk (git) | VCS |
| jj (jujutsu) | Release binary from GitHub | Garage workflow - see [jj-workflow.md](jj-workflow.md) |
| gh | apk (gh) | GitHub CLI |

**Database clients:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| psql | apk (postgresql-client) | PostgreSQL client |

**General tools:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| curl | apk (curl) | HTTP client |
| jq | apk (jq) | JSON processing |
| yq | Release binary from GitHub | YAML processing |
| ripgrep | apk (ripgrep) | Fast search |
| fd | apk (fd) | Fast find |
| bat | apk (bat) | Better cat |
| htop | apk (htop) | Process monitoring |
| tree | apk (tree) | Directory visualization |

**Kubernetes:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| kubectl | apk (kubectl) | K8s CLI |

**AI:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| claude-code | Native binary (shell script) | Claude CLI for wrenching |

Claude Code is installed via the official shell script:
```bash
curl -fsSL https://claude.ai/install.sh | bash
```
This is run during container build. The binary installs to `~/.local/bin/claude`.

**Connectivity:**

| Tool | Installation | Purpose |
|------|--------------|---------|
| wireguard-tools | apk (wireguard-tools) | WireGuard client for tunnel |
| ttyd | Release binary from GitHub | WebSocket terminal daemon |
| tmux | apk (tmux) | Terminal multiplexer for session persistence |

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
- Tools are installed in standard locations (`/usr/bin`, `/usr/local/bin`) in the image, read-only via `readOnlyRootFilesystem`

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

# Telemetry
DO_NOT_TRACK="1"

# TLS
SSL_CERT_FILE="/etc/ssl/certs/ca-bundle.crt"
```

### Security Model

**Philosophy: The container IS the sandbox.**

Inside the garage, Claude Code has full control. Isolation comes from the container and namespace boundary.

**Inside garage (unrestricted):**
- Root access (can install packages via apk, modify anything)
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

Local builds use standard `docker build` with the Dockerfile. Multi-arch builds use `docker buildx`. Architecture is auto-detected.

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

**Both commands must succeed.** Container builds can fail in non-obvious ways (missing packages, broken paths, incorrect env vars) that only surface when actually built.

**Files requiring build verification:**
- `infra/docker/Dockerfile.garage` - Garage container definition
- `infra/docker/*-entrypoint` - Container entrypoint scripts

**Common build failures:**
| Error | Cause | Fix |
|-------|-------|-----|
| `Package not found` | Missing apk package | Check Wolfi package availability, use release binary if needed |
| `command not found` | Missing from PATH | Verify installation path, add to PATH if needed |
| `Permission denied` | Script not executable | Add `chmod +x` in Dockerfile |

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

```dockerfile
# infra/docker/Dockerfile.garage
FROM cgr.dev/chainguard/wolfi-base:latest

# Install core system tools
RUN apk add --no-cache \
    bash coreutils findutils grep sed gawk tar gzip bzip2 xz \
    curl wget git gh jq tree htop procps util-linux ca-certificates tzdata

# Install Rust toolchain
RUN apk add --no-cache \
    rust cargo rustfmt clippy rust-analyzer \
    cargo-watch cargo-nextest mold sccache \
    pkgconf openssl-dev postgresql-dev

# Install database clients
RUN apk add --no-cache postgresql-client

# Install general tools
RUN apk add --no-cache ripgrep fd bat tmux

# Install Kubernetes tools
RUN apk add --no-cache kubectl

# Install WireGuard tools
RUN apk add --no-cache wireguard-tools

# Install Node.js
RUN apk add --no-cache nodejs npm

# Install tools from release binaries (not in Wolfi repos)
RUN curl -Lo /usr/local/bin/jj https://github.com/jj-vcs/jj/releases/download/v0.28.0/jj-v0.28.0-x86_64-unknown-linux-gnu.tar.gz && \
    tar -xzf /usr/local/bin/jj -C /usr/local/bin && \
    chmod +x /usr/local/bin/jj

RUN curl -Lo /usr/local/bin/ttyd https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.x86_64 && \
    chmod +x /usr/local/bin/ttyd

RUN curl -Lo /usr/local/bin/yq https://github.com/mikefarah/yq/releases/download/v4.44.1/yq_linux_amd64 && \
    chmod +x /usr/local/bin/yq

# Install Claude Code
RUN curl -fsSL https://claude.ai/install.sh | bash

# Install sqlx-cli
RUN cargo install sqlx-cli --no-default-features --features postgres

# Set environment variables
ENV HOME=/root \
    TERM=xterm-256color \
    SHELL=/bin/bash \
    WORKSPACE=/workspace \
    CARGO_HOME=/root/.cargo \
    CARGO_TARGET_DIR=/workspace/target \
    RUST_BACKTRACE=1 \
    RUST_LOG=info \
    RUSTFLAGS="-C link-arg=-fuse-ld=mold" \
    RUSTC_WRAPPER=sccache \
    DO_NOT_TRACK=1 \
    SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt

WORKDIR /workspace

CMD ["garage-entrypoint"]
```

The standard Dockerfile format is universally understood and easy to modify.

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

### v0.20 (2026-04-10)
- Replace Nix dockerTools with standard Dockerfile approach (nix-removal.md v0.2)
- Change base image from NixOS to Wolfi (Chainguard) for minimal CVE footprint
- Update philosophy: remove Nix reproducibility, focus on security and universal tooling
- Update project structure: replace `flake.nix`/`flake.lock`/`infra/pkgs`/`infra/modules` with `infra/docker/Dockerfile.garage`
- Update tooling table: replace Nix package references with apk/release binary installation methods
- Update build commands: replace Docker-wrapped Nix with standard `docker build`
- Remove Nix-specific environment variables (NIX_PATH)
- Remove Nix binary cache from allowed egress
- Update example container definition from Nix to Dockerfile format

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
