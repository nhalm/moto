# Container System

| | |
|--------|----------------------------------------------|
| Version | 1.5 |
| Status | Ripping |
| Last Updated | 2026-03-05 |

## Overview

The container system defines how Moto builds, distributes, and runs OCI containers. Two distinct container types serve different purposes: **garage containers** (dev environment for wrenching) and **bike containers** (minimal runtime for ripping).

This spec covers the build pipeline, registry strategy, and lifecycle that connects these two container types.

## Specification

### Philosophy

**The Frame and the Ride**

Think of containers like motorcycle builds:

| Concept | Garage Container | Bike Container |
|---------|------------------|----------------|
| Purpose | Where you wrench | What rips on the road |
| Contents | Full toolbox | Just the engine |
| Size | ~3GB (all tools) | ~50MB (minimal) |
| Runs as | Root (AI needs access) | Non-root (security) |
| Lifecycle | Hours to days | Weeks to months |

**Core principles:**

1. **Reproducible**: Docker builds with pinned base images and locked dependencies (`Cargo.lock`)
2. **Layered**: Multi-stage Dockerfiles create efficient Docker layers
3. **Minimal runtime**: Bike containers exclude all build tooling
4. **Secure by default**: Non-root, no shell in production

### Container Types

```
┌─────────────────────────────────────────────────────────────────┐
│                    Container Build Pipeline                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │ Dockerfile   │                      │  Dockerfile  │         │
│  │   .garage    │                      │    .bike     │         │
│  └──────┬───────┘                      └──────┬───────┘         │
│         │                                     │                  │
│         ▼                                     ▼                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │docker build  │                      │ docker build │         │
│  │ (wolfi-base) │                      │ (wolfi-base) │         │
│  └──────┬───────┘                      └──────┬───────┘         │
│         │                                     │                  │
│         ▼                                     ▼                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │   GARAGE     │                      │    BIKE      │         │
│  │  Container   │                      │  (base image)│         │
│  │   ~3GB       │                      │   <20MB      │         │
│  │(moto-garage) │                      │ (moto-bike)  │         │
│  └──────────────┘                      └──────────────┘         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### Garage Container (`moto-garage`)

The wrenching environment. See `dev-container.md` for full specification.

```
Image: ${MOTO_REGISTRY}/moto-garage
Size: ~2-3GB compressed
User: root
Contains: Full Rust toolchain, all dev tools, Claude Code
```

#### Bike Base Image (`moto-bike`)

The minimal production base image. Engines (binaries) are added to create final deployable images.

```
Image: ${MOTO_REGISTRY}/moto-bike
Size: <20MB compressed
User: 1000:1000 (non-root)
Contains: CA certificates, tzdata, non-root user setup
Excludes: Shell, package manager, libc (engines are static)
```

**Final images:** Each engine gets its own image built from the bike base:
- `moto-bike` + club binary → `moto-club`
- `moto-bike` + keybox-server binary → `moto-keybox`
- `moto-bike` + vault binary → `moto-vault` (future)
- `moto-bike` + proxy binary → `moto-proxy` (future)

See [moto-bike.md](moto-bike.md) for full specification of the bike base image and engine contract.

### Infrastructure Directory Structure

Docker-based structure with Dockerfiles:

```
moto/
├── rust-toolchain.toml          # Pins Rust version
├── .cargo/config.toml           # Cargo settings
└── infra/
    ├── docker/                  # Container definitions
    │   ├── Dockerfile.garage    # Garage container definition
    │   ├── Dockerfile.bike      # Bike base image
    │   ├── Dockerfile.club      # Club engine image
    │   └── Dockerfile.keybox    # Keybox engine image
    └── smoke-test.sh            # Container smoke tests
```

**Why this structure:**

| Benefit | Description |
|---------|-------------|
| Standard | Universal Dockerfile format, no custom build systems |
| Simple | Each Dockerfile is self-contained |
| Portable | Works with any Docker-compatible tooling |
| Transparent | Build steps are explicit, not abstracted |

**Build commands:**

See [makefile.md](makefile.md) for build targets. Local builds use standard `docker build`.

### Build Pipeline Architecture

**Docker multi-stage builds:**

```
Stage 1: Builder        Stage 2: Runtime
─────────────────────   ─────────────────────
FROM wolfi-base         FROM moto-bike (scratch)
Install build tools     COPY binary from builder
cargo build --release   Minimal runtime image
```

**Key insight:** Multi-stage Dockerfiles separate build and runtime environments. The builder stage has the full Rust toolchain; the final image contains only the binary.

**Rust builds use standard cargo.** Engine images (`Dockerfile.club`, `Dockerfile.keybox`) use multi-stage builds: a Wolfi builder stage compiles the Rust binary, then copies it onto the `moto-bike` base. Docker layer caching of the dependency build step provides efficient incremental builds.

### Dockerfile Examples

**Garage container** (`infra/docker/Dockerfile.garage`):

Single-stage build from Wolfi base with all dev tools:

```dockerfile
FROM cgr.dev/chainguard/wolfi-base:latest

# Install tools via apk
RUN apk add --no-cache \
    rust cargo \
    git bash curl \
    kubectl k9s \
    postgresql-client redis \
    ripgrep fd jq

# Install tools from release binaries (not in Wolfi repos)
RUN curl -L https://github.com/jj-vcs/jj/releases/download/v0.15.0/jj-v0.15.0-x86_64-unknown-linux-musl.tar.gz | tar -xz -C /usr/local/bin

WORKDIR /workspace
USER root
ENV RUST_BACKTRACE=1

CMD ["/bin/bash"]
```

**Bike base image** (`infra/docker/Dockerfile.bike`):

Minimal runtime-only image from scratch:

```dockerfile
FROM cgr.dev/chainguard/wolfi-base:latest AS builder

# Extract CA certificates and timezone data
RUN mkdir -p /runtime/etc/ssl/certs /runtime/usr/share/zoneinfo
RUN cp -r /etc/ssl/certs/* /runtime/etc/ssl/certs/
RUN cp -r /usr/share/zoneinfo/* /runtime/usr/share/zoneinfo/

FROM scratch
COPY --from=builder /runtime /

# Create non-root user
COPY --from=builder /etc/passwd /etc/passwd
COPY --from=builder /etc/group /etc/group

USER 1000:1000
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
```

**Engine image** (`infra/docker/Dockerfile.club`):

Multi-stage build: compile with Wolfi, run on bike base:

```dockerfile
# Build stage
FROM cgr.dev/chainguard/wolfi-base:latest AS builder

RUN apk add --no-cache rust cargo build-base openssl-dev

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --bin moto-club

# Runtime stage
FROM moto-bike:latest

COPY --from=builder /build/target/release/moto-club /bin/moto-club

USER 1000:1000
EXPOSE 8080 8081 9090
ENV RUST_BACKTRACE=1

ENTRYPOINT ["/bin/moto-club"]
```

### Image Registry & Tagging Strategy

**Registry is configurable.** Local by default, remote for production.

**Registry configuration:**

| Environment | Registry | Purpose |
|-------------|----------|---------|
| Local dev | `localhost:5050` | Fast iteration, no auth |
| K3s cluster | `registry.moto-system:5000` | In-cluster registry |
| Production | `ghcr.io/<org>` | Public/private remote (optional) |

The active registry is set via environment variable:

```bash
# Local development (default)
export MOTO_REGISTRY="localhost:5050"

# K3s in-cluster
export MOTO_REGISTRY="registry.moto-system:5000"

# Remote (when needed)
export MOTO_REGISTRY="ghcr.io/nhalm"
```

**Local registry setup:**

```bash
# Run local registry (one-time setup)
docker run -d -p 5050:5000 --name moto-registry registry:2

# Or via k3s (built-in option)
# k3s ships with containerd, can use embedded registry
```

**Naming convention:**

```
${MOTO_REGISTRY}/moto-<name>:<tag>

Images:
  moto-garage   → Development container (full toolchain)
  moto-bike     → Production base image (minimal runtime)
  moto-club     → Club engine (bike + club binary)
  moto-keybox   → Keybox engine (bike + keybox-server binary)
  moto-vault    → Vault engine (bike + vault binary) (future)
  moto-proxy    → Proxy engine (bike + proxy binary) (future)
```

**Tagging strategy:**

| Tag | When | Example |
|-----|------|---------|
| `latest` | Every main branch build | `moto-garage:latest` |
| `<sha>` | Every build | `moto-garage:a1b2c3d` |
| `v<semver>` | Git tags only | `moto-club:v1.2.3` |
| `<branch>` | Feature branches (optional) | `moto-garage:feature-tokenization` |

**Example full image references:**

```bash
# Local development
localhost:5050/moto-garage:latest
localhost:5050/moto-bike:latest
localhost:5050/moto-club:a1b2c3d

# K3s cluster
registry.moto-system:5000/moto-club:v1.0.0
registry.moto-system:5000/moto-vault:v1.0.0

# Remote (future/optional)
ghcr.io/nhalm/moto-club:v1.0.0
```

### Local Build Workflow

**Standard Docker builds:** Local builds use `docker build` with standard Dockerfiles. This works on any platform (Mac, Linux, Windows) with Docker installed.

The flow:
1. Run `docker build -f infra/docker/Dockerfile.<image> -t <image-name> .`
2. Image is available in local Docker
3. Optionally push to registry with `docker push`

For multi-architecture builds, use `docker buildx`:

```bash
# Create builder (one-time setup)
docker buildx create --name moto-builder --use

# Build for multiple architectures
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f infra/docker/Dockerfile.club \
  -t localhost:5050/moto-club:latest \
  --push .
```

See [makefile.md](makefile.md) for build targets.

### Smoke Testing

Smoke tests verify containers build correctly and contain expected tools/configuration.

**Garage container tests** (`infra/smoke-test.sh`):

| Check | Verifies |
|-------|----------|
| Core tools | rustc, cargo, git, jj, kubectl present |
| Environment | RUST_BACKTRACE, CARGO_HOME, WORKSPACE set |
| Rust compilation | Can compile and run a simple program |

**Usage:**

```bash
# Build and test via Makefile
make test-garage

# Or run directly
./infra/smoke-test.sh

# Keep container for debugging
./infra/smoke-test.sh --keep
```

**Bike container tests** (future):

Bike containers have no shell, so testing is different:
- Verify binary exists and is executable
- Check exposed ports match spec
- Verify non-root user (UID 1000)

### CI/CD Pipeline (future)

**CI uses docker buildx for multi-architecture builds.**

GitHub Actions uses Docker buildx with layer caching for efficient builds.

```yaml
# .github/workflows/containers.yml
name: Build Containers

on:
  push:
    branches: [main]
    tags: ['v*']
  pull_request:
    branches: [main]

env:
  REGISTRY: ghcr.io
  REGISTRY_ORG: ${{ github.repository_owner }}

jobs:
  build-garage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to registry
        if: github.event_name != 'pull_request'
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Build and push garage image
        uses: docker/build-push-action@v5
        with:
          context: .
          file: infra/docker/Dockerfile.garage
          platforms: linux/amd64,linux/arm64
          push: ${{ github.event_name != 'pull_request' }}
          tags: |
            ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-garage:latest
            ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-garage:${{ github.sha }}
          cache-from: type=gha
          cache-to: type=gha,mode=max

  build-bikes:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        engine: [club, keybox]
    steps:
      - uses: actions/checkout@v4

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Log in to registry
        if: github.event_name != 'pull_request'
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Build and push ${{ matrix.engine }} image
        uses: docker/build-push-action@v5
        with:
          context: .
          file: infra/docker/Dockerfile.${{ matrix.engine }}
          platforms: linux/amd64,linux/arm64
          push: ${{ github.event_name != 'pull_request' }}
          tags: |
            ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-${{ matrix.engine }}:latest
            ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-${{ matrix.engine }}:${{ github.sha }}
          cache-from: type=gha
          cache-to: type=gha,mode=max
```

### Reproducibility Guarantees

**What makes builds reproducible:**

| Input | How it's locked |
|-------|-----------------|
| Base image | Wolfi base image pinned by digest (e.g., `@sha256:abc123...`) |
| Rust toolchain | `rust-toolchain.toml` pins version |
| Rust dependencies | `Cargo.lock` pins exact versions |
| System packages | Wolfi packages pinned to specific versions in Dockerfile |

**Best practices for reproducibility:**

1. **Pin base image by digest:**
   ```dockerfile
   FROM cgr.dev/chainguard/wolfi-base@sha256:abc123...
   ```

2. **Pin package versions:**
   ```dockerfile
   RUN apk add --no-cache rust=1.88.0-r0 cargo=1.88.0-r0
   ```

3. **Lock Rust dependencies:** `Cargo.lock` pins all transitive dependencies

**Note:** Full reproducibility requires additional controls (SOURCE_DATE_EPOCH, --no-cache builds) beyond typical development workflows. For production, use image digests and signatures to verify provenance.

### Multi-Architecture Support

**Supported architectures:**

- `linux/amd64` (Intel/AMD servers, most CI)
- `linux/arm64` (Apple Silicon, Graviton, Raspberry Pi)

**Build strategy:**

Use `docker buildx` for multi-architecture builds:

```bash
# Create builder (one-time setup)
docker buildx create --name moto-builder --use

# Build for multiple architectures
docker buildx build \
  --platform linux/amd64,linux/arm64 \
  -f infra/docker/Dockerfile.club \
  -t ghcr.io/nhalm/moto-club:v1.0.0 \
  --push .
```

**CI multi-arch:**

GitHub Actions uses `docker/build-push-action` with `platforms: linux/amd64,linux/arm64`. This automatically builds both architectures and creates a multi-arch manifest.

The result is a single tag (`moto-club:v1.0.0`) that works on both architectures — Docker automatically pulls the correct variant for the platform.

### Cache Strategy

**Docker layer caching:**

Docker automatically caches layers based on instruction order. Structure Dockerfiles for maximum cache reuse:

```dockerfile
# Layer 1: Base image (rarely changes)
FROM cgr.dev/chainguard/wolfi-base:latest

# Layer 2: System packages (rarely changes)
RUN apk add --no-cache rust cargo build-base

# Layer 3: Dependency manifest (changes less often than code)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release

# Layer 4: Source code (changes frequently)
COPY src ./src
RUN cargo build --release
```

**Rust dependency caching:**

Multi-stage Dockerfiles can cache Rust dependencies separately from application code:

```dockerfile
# Build dependencies layer (cached until Cargo.lock changes)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Build application (uses cached dependencies)
COPY src ./src
RUN cargo build --release
```

**CI caching:**

GitHub Actions uses `cache-from` and `cache-to` for cross-build caching:

```yaml
- uses: docker/build-push-action@v5
  with:
    cache-from: type=gha
    cache-to: type=gha,mode=max
```

This caches layers in GitHub Actions cache storage, speeding up subsequent builds.

### Image Signing (cosign)

Sign images to verify they came from your build pipeline.

**Install cosign:**

```bash
# macOS
brew install cosign

# Linux (binary install)
curl -LO https://github.com/sigstore/cosign/releases/latest/download/cosign-linux-amd64
chmod +x cosign-linux-amd64
sudo mv cosign-linux-amd64 /usr/local/bin/cosign
```

**Keyless signing (recommended for CI):**

Uses OIDC identity from GitHub Actions - no keys to manage:

```bash
# In CI, cosign uses GitHub's OIDC token automatically
cosign sign --yes ${REGISTRY}/moto-club:${SHA}
```

**Local signing with key pair:**

```bash
# Generate key pair (one-time)
cosign generate-key-pair
# Creates cosign.key (private) and cosign.pub (public)

# Sign an image
cosign sign --key cosign.key localhost:5050/moto-club:latest

# Verify an image
cosign verify --key cosign.pub localhost:5050/moto-club:latest
```

**CI workflow addition:**

```yaml
- name: Sign image
  if: github.event_name != 'pull_request'
  env:
    COSIGN_EXPERIMENTAL: "true"  # Enable keyless
  run: |
    cosign sign --yes ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-${{ matrix.engine }}:$SHA
```

**Verification policy (future):**

Deploy admission controller (Kyverno/Gatekeeper) to require signatures:

```yaml
# Example Kyverno policy
apiVersion: kyverno.io/v1
kind: ClusterPolicy
metadata:
  name: verify-moto-images
spec:
  validationFailureAction: enforce
  rules:
  - name: verify-signature
    match:
      resources:
        kinds: [Pod]
    verifyImages:
    - imageReferences: ["*/moto-*"]
      attestors:
      - entries:
        - keyless:
            issuer: "https://token.actions.githubusercontent.com"
            subject: "https://github.com/nhalm/moto/*"
```

### Vulnerability Scanning (trivy)

Scan images for known CVEs before deployment.

**Install trivy:**

```bash
# macOS
brew install trivy

# Linux (binary install)
curl -sfL https://raw.githubusercontent.com/aquasecurity/trivy/main/contrib/install.sh | sh -s -- -b /usr/local/bin

# Or use Docker (no install needed)
docker run --rm -v /var/run/docker.sock:/var/run/docker.sock \
  aquasec/trivy image moto-garage:latest
```

**Local scanning:**

```bash
# Scan with severity filter
trivy image --severity HIGH,CRITICAL moto-club:latest

# Scan and fail on findings (for CI)
trivy image --exit-code 1 --severity CRITICAL moto-club:latest

# Output as JSON for processing
trivy image --format json --output results.json moto-club:latest

# Scan before pushing (recommended flow)
docker build -f infra/docker/Dockerfile.club -t moto-club:latest .
trivy image --exit-code 1 --severity HIGH,CRITICAL moto-club:latest
docker push localhost:5050/moto-club:latest
```

**CI workflow addition:**

```yaml
- name: Scan image
  run: |
    # Install trivy
    curl -sfL https://raw.githubusercontent.com/aquasecurity/trivy/main/contrib/install.sh | sh -s -- -b /usr/local/bin

    # Scan - fail on CRITICAL, warn on HIGH
    trivy image --exit-code 1 --severity CRITICAL moto-${{ matrix.engine }}:latest
    trivy image --severity HIGH moto-${{ matrix.engine }}:latest
```

**Severity policy:**

| Severity | Action | Rationale |
|----------|--------|-----------|
| CRITICAL | Block deploy | Known exploits, must fix |
| HIGH | Warn, review | Significant risk, fix soon |
| MEDIUM | Log only | Track, fix when convenient |
| LOW | Ignore | Minimal risk |

### SBOM Generation

Software Bill of Materials - list all components in the image for compliance and auditing.

**Generate SBOM with trivy:**

```bash
# SPDX format (widely supported)
trivy image --format spdx-json --output sbom.spdx.json moto-club:latest

# CycloneDX format (good for security tools)
trivy image --format cyclonedx --output sbom.cdx.json moto-club:latest
```

**Alternative: syft (from Anchore):**

```bash
# Install
brew install syft

# Generate SBOM
syft moto-club:latest -o spdx-json > sbom.spdx.json
```

**Attach SBOM to image as attestation:**

```bash
# Sign and attach SBOM
cosign attest --predicate sbom.spdx.json --type spdx \
  localhost:5050/moto-club:latest

# Verify attestation exists
cosign verify-attestation --type spdx \
  localhost:5050/moto-club:latest
```

**CI workflow addition:**

```yaml
- name: Generate and attach SBOM
  run: |
    # Generate SBOM
    trivy image --format spdx-json --output sbom.spdx.json \
      moto-${{ matrix.engine }}:latest

    # Attach as attestation (if signing enabled)
    cosign attest --yes --predicate sbom.spdx.json --type spdx \
      ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-${{ matrix.engine }}:$SHA
```

**What SBOM captures:**

- All packages and versions in the image
- Dependency tree
- License information
- Source locations (where available)

Useful for:
- Compliance audits (SOC 2, PCI DSS)
- Incident response (is vulnerable library X in our images?)
- License compliance (do we have GPL in our images?)

### Local Development

See [makefile.md](makefile.md) for build targets (`build-garage`, `test-garage`, etc.).

### Image Size Targets

| Image | Target | Rationale |
|-------|--------|-----------|
| `moto-garage` | < 3GB | Full toolchain acceptable for dev |
| `moto-bike` | < 20MB | Minimal base (just certs, user) |
| `moto-club`, etc. | < 50MB | Bike base + single engine binary |

**Size optimization for bikes:**

1. **Single binary**: No shell, no package manager
2. **Symbol stripping**: Rust's `strip = true` in release profile
3. **Release mode**: `cargo build --release` with LTO
4. **Minimal deps**: Only cacert for TLS

```toml
# Release profile in Cargo.toml
[profile.release]
lto = true
codegen-units = 1
strip = true
```

### Security Considerations

**Garage (dev) containers:**

- Run as root (AI needs full access)
- Security comes from namespace/network isolation
- No secrets baked in (fetched from keybox)

**Bike (production) containers:**

- Run as non-root (UID 1000)
- No shell or package manager
- Minimal attack surface
- Read-only filesystem where possible
- Secrets via K8s secrets or keybox

**Image hygiene:**

- No credentials in image layers
- No `.git` directory in images
- No development dependencies in production images
- Regular vulnerability scanning (TODO)

### Relationship to Other Specs

| Spec | Relationship |
|------|--------------|
| `dev-container.md` | Defines garage container contents |
| `moto-bike.md` | Defines bike base image and engine contract |
| `moto-club.md` | Club runs in bike container |
| `garage-lifecycle.md` | Manages garage container instances |
| `local-cluster.md` | Where containers run |

### Troubleshooting

**Build failures:**

```bash
# View detailed Docker build logs
docker build --progress=plain -f infra/docker/Dockerfile.garage -t moto-garage .

# Keep failed build for inspection
docker build --progress=plain --rm=false -f infra/docker/Dockerfile.garage -t moto-garage .

# Check image size breakdown
docker images moto-garage:latest
docker history moto-garage:latest
```

**Image inspection:**

```bash
# View layers and sizes
docker history moto-garage:latest

# Deep dive with dive tool
dive moto-garage:latest

# Export and inspect filesystem
docker save moto-garage:latest | tar -tvf -

# Compare two image versions
docker history moto-garage:v1 > v1.txt
docker history moto-garage:v2 > v2.txt
diff v1.txt v2.txt
```

**Debugging bike containers (no shell):**

Bike containers have no shell. Use ephemeral debug containers:

```bash
# Attach debug container to running pod
kubectl debug -it moto-club-abc123 --image=busybox --target=club

# Or run the image with shell override (for local testing)
# This won't work because there's no shell, but you can:
docker run --rm -it --entrypoint="" moto-club:latest /bin/sh
# Error: executable file not found

# Instead, use a sidecar approach or copy binary out:
docker create --name temp moto-club:latest
docker cp temp:/bin/club ./club
docker rm temp
# Now inspect the binary locally
```

**Registry issues:**

```bash
# Test registry connectivity
curl -s http://localhost:5050/v2/_catalog

# Check if image exists
curl -s http://localhost:5050/v2/moto-garage/tags/list

# Registry logs (if using container)
docker logs moto-registry
```

**Common errors:**

| Error | Cause | Solution |
|-------|-------|----------|
| `OOM killed during build` | Rust compilation needs memory | Increase Docker memory limit or use remote builder |
| `registry unreachable` | Registry not running | `make registry-start` |
| `image not found` | Not built yet | Run `docker build` first |
| `permission denied` | Non-root in bike container | Check file ownership in image |
| `no such file` in container | Binary path wrong | Check `Entrypoint` in image config |
| `failed to solve` | Dockerfile syntax error | Check Dockerfile syntax and COPY paths |

**Verifying image contents:**

```bash
# Inspect image layers
docker history moto-club:latest

# Check image digest
docker inspect moto-club:latest | jq '.[0].RepoDigests'

# Extract and inspect binary
docker create --name temp moto-club:latest
docker cp temp:/bin/moto-club ./moto-club
docker rm temp
file ./moto-club
```

## Changelog

### v1.5 (2026-03-05)
- Fix: Registry example port mapping: `-p 5050:5050` → `-p 5050:5000` (registry:2 listens on 5000 internally)
- Fix: Flake example `nixpkgs.url`: `nixos-24.05` → `nixos-unstable` to match actual flake.nix
- Fix: Flake example `crane` input: remove stale `inputs.nixpkgs.follows` (newer crane doesn't have nixpkgs input)
- Fix: Flake example `eachSystem`: use `eachDefaultSystem` (includes darwin) with `isLinux` guard for container packages
- Fix: Flake example `commonArgs.buildInputs`: remove `postgresql.lib` (only in devShell, not needed for engine builds)

### v1.4 (2026-03-05)
- Docs: Mark CI/CD Pipeline section as `(future)` — `.github/workflows/` not yet implemented
- Fix: `make registry-start` uses port 5000 but should use 5050 (see bug-fix.md)

### v1.3 (2026-02-28)
- Bump Rust toolchain from 1.85 to 1.88 (`home` crate v0.5.12 requires Rust 1.88)
- Add `stdenv.cc` and `lld` to `commonArgs.nativeBuildInputs` (crane needs a C compiler/linker; `.cargo/config.toml` specifies `-fuse-ld=lld` for Linux targets)

### v1.2 (2026-02-27)
- Switch engine builds from `rustPlatform.buildRustPackage` (manual `cargoHash`) to crane (`craneLib.buildPackage`)
- Crane reads `Cargo.lock` directly — no manual hash updates when dependencies change
- Add `crane` flake input to `flake.nix`; pass `craneLib`, `commonArgs`, and `cargoArtifacts` to engine packages
- Engine packages (`moto-club.nix`, `moto-keybox.nix`) receive crane args and use `craneLib.buildPackage`
- Remove `cargoHash` from engine packages entirely
- Dependencies are built once via `craneLib.buildDepsOnly` and shared across all engine builds

### v1.1 (2026-02-24)
- Docs: Update registry port from 5000 to 5050 (matches local-cluster.md v0.3)

### v1.0 (2026-02-18)
- Add `moto-keybox` to final images list (bike + keybox-server binary)
- Add `infra/pkgs/moto-keybox.nix` to directory structure
- Update directory structure to match implementation (terminal.nix, moto-bike.nix, moto-club.nix)
- Mark moto-vault and moto-proxy as (future)
- Add build-club, push-club, build-keybox, push-keybox Makefile targets (see makefile.md)

### v0.9 (2026-01-28)
- Clarified container naming: `moto-bike` is base image, `moto-club`/`moto-vault`/`moto-proxy` are final images
- Renamed `mkEngine` → `mkBike` helper, produces `moto-{name}` not `moto-engine-{name}`
- Added `moto-bike` base image definition (minimal: CA certs, non-root user)
- Final images now use `fromImage = moto-bike` layering
- Updated all examples and CI workflows to use new naming
- See moto-bike.md for full specification

### v0.8 (2026-01-26)
- Switch to flake-parts for modular flake organization
- Move from `infra/pkgs/*.nix` + `infra/modules/*.nix` to `infra/*.nix` flake-parts modules
- Use `buildEnv` for package composition (avoids file collisions like cacert)
- Use `buildLayeredImage` with `buildEnv` wrapper for collision-free builds

### v0.7 (2026-01-26)
- Mark crane flake example as reference/aspirational (not current implementation)

### v0.6 (2026-01-26)
- Docker-wrapped Nix approach: run `nix build` inside `nixos/nix` container
- Works on Mac without Linux builder, keeps Nix flake as single source of truth
- No separate Dockerfile needed
- CI uses direct `nix build` on Linux runners
- See makefile.md for build targets

### v0.4 (2026-01-25)
- Switch garage container from Nix dockerTools to Dockerfile-based builds
- Garage builds work on Mac (Docker/Colima) without linux-builder setup
- Multi-arch support via docker buildx (amd64 + arm64)
- Bike containers still use Nix dockerTools (built in CI on Linux)
- Update directory structure: `infra/Dockerfile.moto-garage`

### v0.3 (2026-01-24)
- Add infra directory structure: `pkgs/`, `modules/`, `machines/`
- Rename `moto-dev` to `moto-garage` for metaphor consistency
- Introduced `moto-bike` base image concept (see moto-bike.md)
- Update all build commands and Makefile targets
- Add PATH note for dockerTools containers

### v0.2 (2026-01-23)
- Add smoke testing section
- Add `docker-test-*` and `docker-shell-*` Makefile targets

### v0.1 (2026-01-20)
- Initial specification

## Notes

- Wolfi base images provide minimal CVE footprint with daily package rebuilds
- `cargo audit` and `cargo deny` should be run in CI as separate jobs
- Consider using distroless or scratch-based final images for smallest attack surface

## References

- [Wolfi (Chainguard)](https://github.com/wolfi-dev) - Minimal container OS with daily CVE fixes
- [Docker multi-stage builds](https://docs.docker.com/build/building/multi-stage/) - Official Docker documentation
- [Docker buildx](https://docs.docker.com/buildx/working-with-buildx/) - Multi-architecture builds
- [OCI Image Spec](https://github.com/opencontainers/image-spec)
