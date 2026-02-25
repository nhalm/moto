# Container System

| | |
|--------|----------------------------------------------|
| Version | 1.0 |
| Status | Ready to Rip |
| Last Updated | 2026-02-18 |

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

1. **Reproducible**: Nix dockerTools builds guarantee identical images from identical inputs
2. **Layered**: `buildLayeredImage` creates efficient Docker layers automatically
3. **Minimal runtime**: Bike containers exclude all build tooling
4. **Secure by default**: Non-root, no shell in production

### Container Types

```
┌─────────────────────────────────────────────────────────────────┐
│                    Container Build Pipeline                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │  flake.nix   │                      │   flake.nix  │         │
│  │ + modules/   │                      │ + Cargo.toml │         │
│  └──────┬───────┘                      └──────┬───────┘         │
│         │                                     │                  │
│         ▼                                     ▼                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │ nix build    │                      │ nix build    │         │
│  │ (dockerTools)│                      │ (dockerTools)│         │
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

Modular structure with packages and modules:

```
moto/
├── flake.nix                    # Root flake - devShells + imports infra/pkgs
├── flake.lock                   # Pinned dependencies
└── infra/
    ├── pkgs/                    # Container package definitions
    │   ├── default.nix          # Exports all packages
    │   ├── moto-garage.nix      # Garage container definition
    │   ├── moto-bike.nix        # Bike base image + mkBike helper
    │   ├── moto-club.nix        # Club engine image (bike + binary)
    │   └── moto-keybox.nix      # Keybox engine image (bike + binary)
    ├── modules/                 # Reusable module components
    │   ├── base.nix             # Core system tools
    │   ├── dev-tools.nix        # Development tooling
    │   ├── terminal.nix         # ttyd + tmux
    │   └── wireguard.nix        # WireGuard tools
    └── smoke-test.sh            # Container smoke tests
```

**Why this structure:**

| Benefit | Description |
|---------|-------------|
| Modular | Each `infra/modules/*.nix` provides a reusable component |
| No collisions | Uses `buildEnv` in pkgs - Nix handles file conflicts |
| Simple | Direct imports, no framework dependencies |
| Composable | Modules return `{ contents, env }` for easy merging |

**Build commands:**

See [makefile.md](makefile.md) for build targets. Local builds use Docker-wrapped Nix.

### Build Pipeline Architecture

**Three-stage pipeline:**

```
Stage 1: Source         Stage 2: Build           Stage 3: Package
─────────────────────   ─────────────────────    ─────────────────────
flake.nix               nix build                dockerTools.buildLayeredImage
flake.lock              cargo build --release    or
Cargo.toml              (inside Nix)             streamLayeredImage
Cargo.lock
src/**
```

**Key insight:** Nix handles the build-vs-runtime separation automatically. The build stage has full toolchain; the output is a minimal closure.

### Complete Flake Example (Reference)

> **Note:** This is a reference implementation showing the ideal pattern with crane for cached Rust builds. The current implementation uses a simpler approach without crane. Consider adopting crane when build times become a bottleneck.

Full `flake.nix` with crane for cached Rust builds:

```nix
{
  description = "Moto - fintech infrastructure";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        # Pin Rust version
        rustToolchain = pkgs.rust-bin.stable."1.83.0".default;
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Common source filtering
        src = craneLib.cleanCargoSource ./.;

        # Build deps once (cached separately)
        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = with pkgs; [ openssl postgresql.lib ];
          nativeBuildInputs = with pkgs; [ pkg-config ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        # Individual engine builds
        moto-club = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p moto-club";
        });

        moto-vault = craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          cargoExtraArgs = "-p moto-vault";
        });

        # Garage dev container
        garage = pkgs.dockerTools.buildLayeredImage {
          name = "moto-garage";
          tag = "latest";
          contents = with pkgs; [
            # From dev-container.md - full toolchain
            rustToolchain
            cargo-watch cargo-nextest cargo-audit cargo-deny
            git jujutsu gh
            postgresql redis
            curl jq yq ripgrep fd
            kubectl k9s
            bashInteractive coreutils tini cacert
          ];
          config = {
            Entrypoint = [ "${pkgs.tini}/bin/tini" "--" ];
            Cmd = [ "${pkgs.bashInteractive}/bin/bash" ];
            WorkingDir = "/workspace";
            User = "root";
            Env = [
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "RUST_BACKTRACE=1"
            ];
          };
        };

        # Bike base image
        moto-bike = pkgs.dockerTools.buildLayeredImage {
          name = "moto-bike";
          tag = "latest";
          contents = [ pkgs.cacert ];
          config = {
            User = "1000:1000";
            Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
          };
          fakeRootCommands = ''
            ${pkgs.dockerTools.shadowSetup}
            groupadd -g 1000 moto
            useradd -u 1000 -g moto -d / -s /sbin/nologin moto
          '';
          enableFakechroot = true;
        };

        # Final image builder: bike base + engine binary
        mkBike = { name, package }: pkgs.dockerTools.buildLayeredImage {
          name = "moto-${name}";
          tag = "latest";
          fromImage = moto-bike;
          contents = [ package ];
          config = {
            Entrypoint = [ "${package}/bin/${name}" ];
            User = "1000:1000";
            ExposedPorts = { "8080/tcp" = {}; "8081/tcp" = {}; "9090/tcp" = {}; };
          };
        };

      in {
        packages = {
          inherit moto-club moto-vault moto-bike;
          moto-garage = garage;
          # Final images built with mkBike
          moto-club-image = mkBike { name = "club"; package = moto-club; };
          moto-vault-image = mkBike { name = "vault"; package = moto-vault; };
          default = moto-club;
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [ cargo-watch rust-analyzer ];
        };

        # Checks run by `nix flake check`
        checks = {
          inherit moto-club moto-vault;
          clippy = craneLib.cargoClippy (commonArgs // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          });
          fmt = craneLib.cargoFmt { inherit src; };
          audit = craneLib.cargoAudit { inherit src; advisory-db = inputs.advisory-db; };
        };
      }
    );
}
```

**Key points:**
- `eachSystem` limited to Linux (containers don't build on macOS)
- `crane.buildDepsOnly` caches dependency compilation
- `mkEngine` helper creates consistent bike containers
- `checks` runs clippy, fmt, audit via `nix flake check`

### Nix Image Building

**Preferred approach:** Pure Nix with `dockerTools.buildLayeredImage`.

Why layers matter:
```
Layer 1: System libraries (glibc, openssl)     ← rarely changes
Layer 2: CA certificates                        ← rarely changes
Layer 3: Application binary                     ← changes per build
```

**Garage image** (defined in `infra/pkgs/moto-garage.nix`):

```nix
# infra/pkgs/moto-garage.nix
{ pkgs, rustToolchain }:
let
  # Import modules
  base = import ../modules/base.nix { inherit pkgs; };
  devTools = import ../modules/dev-tools.nix { inherit pkgs rustToolchain; };
  # ... other modules

  allContents = base.contents ++ devTools.contents;

  # Use buildEnv to handle file collisions
  garageEnv = pkgs.buildEnv {
    name = "garage-env";
    paths = allContents;
  };
in
pkgs.dockerTools.buildLayeredImage {
  name = "moto-garage";
  tag = "latest";
  contents = [ garageEnv ];
  config = {
    Cmd = [ "/bin/bash" ];
    WorkingDir = "/workspace";
    Env = base.env ++ devTools.env;
  };
}
```

**Key points:**
- `buildLayeredImage` produces tarball for `docker load`
- `buildEnv` handles file collisions automatically (required for packages with overlapping dirs like `share/`)
- Modules in `infra/modules/` provide reusable package sets

**Bike base + final images** (defined in `infra/bike.nix` flake-parts module):

```nix
# infra/flake/bike.nix
{ inputs, ... }: {
  perSystem = { pkgs, self', ... }:
  let
    # Bike base image - minimal runtime
    moto-bike = pkgs.dockerTools.buildLayeredImage {
      name = "moto-bike";
      tag = "latest";
      contents = [ pkgs.cacert ];
      config = {
        User = "1000:1000";
        Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
      };
      fakeRootCommands = ''
        ${pkgs.dockerTools.shadowSetup}
        groupadd -g 1000 moto
        useradd -u 1000 -g moto -d / -s /sbin/nologin moto
      '';
      enableFakechroot = true;
    };

    # Build final image: bike base + engine binary
    mkBike = { name, package }: pkgs.dockerTools.buildLayeredImage {
      name = "moto-${name}";
      tag = "latest";
      fromImage = moto-bike;
      contents = [ package ];
      config = {
        Entrypoint = [ "${package}/bin/${name}" ];
        User = "1000:1000";
        ExposedPorts = {
          "8080/tcp" = {};
          "8081/tcp" = {};
          "9090/tcp" = {};
        };
        WorkingDir = "/";
        Env = [ "RUST_BACKTRACE=1" ];
      };
    };
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
docker run -d -p 5050:5050 --name moto-registry registry:2

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

**Docker-wrapped Nix:** Local builds run `nix build` inside a `nixos/nix` container. This works on any platform (Mac or Linux) without requiring a Linux builder configuration.

The flow:
1. Docker runs a `nixos/nix` container with the repo mounted
2. Inside the container, `nix build .#moto-garage` runs
3. Output is loaded via `docker load`
4. Image is pushed to local registry

Architecture is auto-detected - ARM Mac builds `aarch64-linux`, Intel builds `x86_64-linux`.

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

### CI/CD Pipeline

**CI uses direct Nix, not Makefile.**

GitHub Actions installs Nix on Linux runners and runs `nix build` directly. This avoids Docker-in-Docker and is faster than the local Docker-wrapped approach.

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

      - uses: cachix/install-nix-action@v27
        with:
          extra_nix_config: |
            experimental-features = nix-command flakes

      - uses: cachix/cachix-action@v15
        with:
          name: moto
          authToken: '${{ secrets.CACHIX_AUTH_TOKEN }}'
        if: ${{ secrets.CACHIX_AUTH_TOKEN != '' }}

      - name: Build garage image
        run: nix build .#moto-garage

      - name: Load image
        run: docker load < result

      - name: Log in to registry
        if: github.event_name != 'pull_request'
        uses: docker/login-action@v3
        with:
          registry: ${{ env.REGISTRY }}
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Push image
        if: github.event_name != 'pull_request'
        run: |
          SHA=$(git rev-parse --short HEAD)
          docker tag moto-garage:latest ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-garage:$SHA
          docker tag moto-garage:latest ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-garage:latest
          docker push ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-garage:$SHA
          docker push ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-garage:latest

  build-bikes:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        engine: [club, vault, proxy]
    steps:
      - uses: actions/checkout@v4

      - uses: cachix/install-nix-action@v27
        with:
          extra_nix_config: |
            experimental-features = nix-command flakes

      - name: Build moto-${{ matrix.engine }} image
        run: nix build .#moto-${{ matrix.engine }}-image

      - name: Load and push
        if: github.event_name != 'pull_request'
        run: |
          docker load < result
          SHA=$(git rev-parse --short HEAD)
          docker tag moto-${{ matrix.engine }}:latest \
            ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-${{ matrix.engine }}:$SHA
          docker push ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-${{ matrix.engine }}:$SHA
```

### Reproducibility Guarantees

**What makes builds reproducible:**

| Input | How it's locked |
|-------|-----------------|
| Nix packages | `flake.lock` pins exact nixpkgs commit |
| Rust toolchain | `rust-toolchain.toml` pins version |
| Rust dependencies | `Cargo.lock` pins exact versions |
| Build environment | Nix ensures identical build closure |

**Verification:**

```bash
# Two builds from same commit should produce identical images
nix build .#moto-club-image
sha256sum result
# → same hash every time on same commit
```

**Content-addressable builds:**

Nix store paths include content hash:
```
/infra/store/abc123...-moto-club-1.0.0
            ^^^^^^^
            content hash
```

### Multi-Architecture Support

**Supported architectures:**

- `linux/amd64` (Intel/AMD servers, most CI)
- `linux/arm64` (Apple Silicon, Graviton, Raspberry Pi)

**Build strategy:**

```nix
# flake.nix uses eachDefaultSystem
flake-utils.lib.eachDefaultSystem (system: ...)

# Produces outputs for:
# - x86_64-linux
# - aarch64-linux
# - x86_64-darwin (dev only)
# - aarch64-darwin (dev only)
```

**CI multi-arch:**

For releases, GitHub Actions builds both architectures and creates a multi-arch manifest:

```bash
# Single tag, multiple architectures
docker manifest create ghcr.io/nhalm/moto-club:v1.0.0 \
  ghcr.io/nhalm/moto-club:v1.0.0-amd64 \
  ghcr.io/nhalm/moto-club:v1.0.0-arm64
```

### Cache Strategy

**Build caches:**

| Cache | Location | Purpose |
|-------|----------|---------|
| Nix store | Cachix (`moto.cachix.org`) | Built Nix derivations |
| Cargo deps | Crane `buildDepsOnly` | Rust dependency artifacts |

**Note:** sccache does NOT work with Nix sandboxed builds. Use `crane` for Rust caching:

```nix
# Using crane for cached Rust builds
let
  craneLib = crane.lib.${system};

  # Build dependencies separately (cached)
  cargoArtifacts = craneLib.buildDepsOnly {
    src = ./.;
  };

  # Build package using cached deps
  moto-club = craneLib.buildPackage {
    inherit cargoArtifacts;
    src = ./.;
  };
in { ... }
```

**Layer caching:**

`buildLayeredImage` creates separate layers:
```
Layer 1: glibc, openssl     (shared across images)
Layer 2: cacert              (shared across images)
Layer 3: application binary  (unique per image)
```

**CI caching:**

```yaml
- uses: cachix/cachix-action@v15
  with:
    name: moto
    # Pulls from cache, pushes new builds
    # Use read-only for PRs to prevent cache poisoning
```

### Image Signing (cosign)

Sign images to verify they came from your build pipeline.

**Install cosign:**

```bash
# macOS
brew install cosign

# Nix
nix-shell -p cosign
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

# Nix
nix-shell -p trivy

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
nix build .#moto-club-image && docker load < result
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
2. **Symbol stripping**: `strip` or Nix's `removeReferencesTo`
3. **Release mode**: `cargo build --release` with LTO
4. **Minimal deps**: Only cacert for TLS

```nix
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
# View detailed Nix build logs
nix build .#moto-garage -L

# Keep failed build for inspection
nix build .#moto-garage --keep-failed

# Check what's in a derivation
nix path-info -rsh .#moto-garage
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
| `image not found` | Not loaded into Docker | `docker load < result` after nix build |
| `permission denied` | Non-root in bike container | Check file ownership in image |
| `no such file` in container | Binary path wrong | Check `Entrypoint` in image config |

**Verifying reproducibility:**

```bash
# Build twice and compare
nix build .#moto-club-image -o result1
nix build .#moto-club-image -o result2
diff <(sha256sum result1) <(sha256sum result2)
# Should be identical

# Check derivation hash
nix path-info --json .#moto-club-image | jq '.[] | .path'
```

## Changelog

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

- Consider distroless base for bike containers (though Nix already builds minimal images)
- `cargo audit` and `cargo deny` are included in the flake's `checks` output
- Run `nix flake check` in CI for Nix validation (clippy, fmt, audit)

## References

- [Nix dockerTools](https://nixos.org/manual/nixpkgs/stable/#sec-pkgs-dockerTools)
- [Loom container system](https://github.com/ghuntley/loom) - reference architecture
- [OCI Image Spec](https://github.com/opencontainers/image-spec)
