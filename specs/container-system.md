# Container System

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Last Updated | 2026-01-20 |

## Overview

The container system defines how Moto builds, distributes, and runs OCI containers. Two distinct container types serve different purposes: **garage containers** (dev environment for wrenching) and **bike containers** (minimal runtime for ripping).

This spec covers the build pipeline, registry strategy, and lifecycle that connects these two container types.

## Jobs to Be Done

- [x] Define container philosophy and types
- [x] Define build pipeline architecture
- [x] Define Nix-based image building
- [x] Define registry and tagging strategy
- [x] Define CI/CD pipeline
- [x] Define reproducibility guarantees
- [x] Define multi-arch build strategy
- [x] Define cache strategy
- [x] Define image signing and verification (cosign)
- [x] Define vulnerability scanning integration (trivy)
- [x] Define SBOM generation
- [x] Add complete flake.nix with crane integration

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

1. **Reproducible**: Nix builds guarantee identical images from identical inputs
2. **Layered**: Changes to code don't rebuild system packages
3. **Minimal runtime**: Bike containers exclude all build tooling
4. **Secure by default**: Non-root, no shell in production

### Container Types

```
┌─────────────────────────────────────────────────────────────────┐
│                    Container Build Pipeline                      │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │   flake.nix  │                      │   Cargo.toml │         │
│  └──────┬───────┘                      └──────┬───────┘         │
│         │                                     │                  │
│         ▼                                     ▼                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │ Nix Build    │                      │ Cargo Build  │         │
│  │ (dev deps)   │                      │ (--release)  │         │
│  └──────┬───────┘                      └──────┬───────┘         │
│         │                                     │                  │
│         ▼                                     ▼                  │
│  ┌──────────────┐                      ┌──────────────┐         │
│  │   GARAGE     │                      │    BIKE      │         │
│  │  Container   │                      │  Container   │         │
│  │   ~3GB       │                      │   ~50MB      │         │
│  │  (moto-dev)  │                      │ (moto-engine)│         │
│  └──────────────┘                      └──────────────┘         │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

#### Garage Container (`moto-dev`)

The wrenching environment. See `dev-container.md` for full specification.

```
Image: ${MOTO_REGISTRY}/moto-dev
Size: ~2-3GB compressed
User: root
Contains: Full Rust toolchain, all dev tools, Claude Code
```

#### Bike Container (`moto-engine-*`)

The production runtime. Minimal, secure, fast to deploy.

```
Image: ${MOTO_REGISTRY}/moto-engine-<name>
Size: ~20-50MB compressed
User: 1000:1000 (non-root)
Contains: Single binary + CA certificates
```

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

### Complete Flake Example

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
          name = "moto-dev";
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

        # Bike container builder
        mkEngine = { name, package }: pkgs.dockerTools.buildLayeredImage {
          name = "moto-engine-${name}";
          tag = "latest";
          contents = [ package pkgs.cacert ];
          config = {
            Entrypoint = [ "${package}/bin/${name}" ];
            User = "1000:1000";
            ExposedPorts = { "8080/tcp" = {}; "8081/tcp" = {}; "9090/tcp" = {}; };
            Env = [ "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt" ];
          };
          fakeRootCommands = ''
            ${pkgs.dockerTools.shadowSetup}
            groupadd -g 1000 moto
            useradd -u 1000 -g moto -d / -s /sbin/nologin moto
          '';
          enableFakechroot = true;
        };

      in {
        packages = {
          inherit moto-club moto-vault garage;
          engine-club = mkEngine { name = "moto-club"; package = moto-club; };
          engine-vault = mkEngine { name = "moto-vault"; package = moto-vault; };
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

**Garage image** (see `dev-container.md` for full nix):

```nix
packages.garage = pkgs.dockerTools.buildLayeredImage {
  name = "moto-dev";
  tag = "latest";
  contents = commonPackages ++ [ pkgs.cacert ];
  config = {
    User = "root";
    # ... full dev environment
  };
};
```

**Bike image:**

```nix
# nix/images/bike.nix
{ pkgs, engine }:

pkgs.dockerTools.buildLayeredImage {
  name = "moto-engine-${engine.name}";
  tag = "latest";

  # Minimal contents - just the binary and TLS certs
  contents = [
    engine.package        # The compiled Rust binary
    pkgs.cacert           # CA certificates for TLS
  ];

  config = {
    Entrypoint = [ "${engine.package}/bin/${engine.name}" ];

    # Non-root for security
    User = "1000:1000";

    # Standard port
    ExposedPorts = {
      "8080/tcp" = {};
    };

    WorkingDir = "/";

    Env = [
      "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
      "RUST_BACKTRACE=1"
    ];
  };

  # Create non-root user properly (not extraCommands which runs as build user)
  # Note: Nix derivation hash already provides reproducibility - no need to
  # override timestamp. Real build time is more useful operationally.
  fakeRootCommands = ''
    ${pkgs.dockerTools.shadowSetup}
    groupadd -g 1000 moto
    useradd -u 1000 -g moto -d / -s /sbin/nologin moto
  '';
  enableFakechroot = true;
}
```

### Image Registry & Tagging Strategy

**Registry is configurable.** Local by default, remote for production.

**Registry configuration:**

| Environment | Registry | Purpose |
|-------------|----------|---------|
| Local dev | `localhost:5000` | Fast iteration, no auth |
| K3s cluster | `registry.moto-system:5000` | In-cluster registry |
| Production | `ghcr.io/<org>` | Public/private remote (optional) |

The active registry is set via environment variable:

```bash
# Local development (default)
export MOTO_REGISTRY="localhost:5000"

# K3s in-cluster
export MOTO_REGISTRY="registry.moto-system:5000"

# Remote (when needed)
export MOTO_REGISTRY="ghcr.io/nhalm"
```

**Local registry setup:**

```bash
# Run local registry (one-time setup)
docker run -d -p 5000:5000 --name moto-registry registry:2

# Or via k3s (built-in option)
# k3s ships with containerd, can use embedded registry
```

**Naming convention:**

```
${MOTO_REGISTRY}/moto-<type>[-<name>]:<tag>

Types:
  moto-dev           → Garage container
  moto-engine-club   → Club service (production)
  moto-engine-vault  → Vault service (production)
  moto-engine-proxy  → Proxy service (production)
```

**Tagging strategy:**

| Tag | When | Example |
|-----|------|---------|
| `latest` | Every main branch build | `moto-dev:latest` |
| `<sha>` | Every build | `moto-dev:a1b2c3d` |
| `v<semver>` | Git tags only | `moto-engine-club:v1.2.3` |
| `<branch>` | Feature branches (optional) | `moto-dev:feature-tokenization` |

**Example full image references:**

```bash
# Local development
localhost:5000/moto-dev:latest
localhost:5000/moto-engine-club:a1b2c3d

# K3s cluster
registry.moto-system:5000/moto-dev:latest
registry.moto-system:5000/moto-engine-club:v1.0.0

# Remote (future/optional)
ghcr.io/nhalm/moto-engine-club:v1.0.0
```

### Local Build Workflow

**Primary workflow: Build and load locally.**

```bash
# Build garage container
nix build .#garage
docker load < result
# → moto-dev:latest loaded

# Build engine container
nix build .#engine-club
docker load < result
# → moto-engine-club:latest loaded

# Tag and push to local registry
docker tag moto-dev:latest localhost:5000/moto-dev:latest
docker push localhost:5000/moto-dev:latest

# Or use Makefile (recommended)
make docker-build          # Build all images
make docker-push-local     # Push to localhost:5000
```

**Makefile targets:**

```makefile
REGISTRY ?= localhost:5000
SHA := $(shell git rev-parse --short HEAD)

.PHONY: docker-build docker-push-local

docker-build: docker-build-garage docker-build-engines

docker-build-garage:
	nix build .#garage
	docker load < result

docker-build-engines:
	nix build .#engine-club && docker load < result
	nix build .#engine-vault && docker load < result

docker-push-local: docker-build
	docker tag moto-dev:latest $(REGISTRY)/moto-dev:latest
	docker tag moto-dev:latest $(REGISTRY)/moto-dev:$(SHA)
	docker push $(REGISTRY)/moto-dev:latest
	docker push $(REGISTRY)/moto-dev:$(SHA)
	@# Repeat for engines...
```

### CI/CD Pipeline (Future)

**For remote registry (when needed):**

GitHub Actions workflow for pushing to ghcr.io or other remote registries. This is optional - local development doesn't require it.

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
        run: nix build .#garage

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
          docker tag moto-dev:latest ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-dev:$SHA
          docker tag moto-dev:latest ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-dev:latest
          docker push ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-dev:$SHA
          docker push ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-dev:latest

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

      - name: Build ${{ matrix.engine }} image
        run: nix build .#engine-${{ matrix.engine }}

      - name: Load and push
        if: github.event_name != 'pull_request'
        run: |
          docker load < result
          SHA=$(git rev-parse --short HEAD)
          docker tag moto-engine-${{ matrix.engine }}:latest \
            ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-engine-${{ matrix.engine }}:$SHA
          docker push ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-engine-${{ matrix.engine }}:$SHA
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
nix build .#engine-club
sha256sum result
# → same hash every time on same commit
```

**Content-addressable builds:**

Nix store paths include content hash:
```
/nix/store/abc123...-moto-club-1.0.0
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
docker manifest create ghcr.io/nhalm/moto-engine-club:v1.0.0 \
  ghcr.io/nhalm/moto-engine-club:v1.0.0-amd64 \
  ghcr.io/nhalm/moto-engine-club:v1.0.0-arm64
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
cosign sign --yes ${REGISTRY}/moto-engine-club:${SHA}
```

**Local signing with key pair:**

```bash
# Generate key pair (one-time)
cosign generate-key-pair
# Creates cosign.key (private) and cosign.pub (public)

# Sign an image
cosign sign --key cosign.key localhost:5000/moto-engine-club:latest

# Verify an image
cosign verify --key cosign.pub localhost:5000/moto-engine-club:latest
```

**CI workflow addition:**

```yaml
- name: Sign image
  if: github.event_name != 'pull_request'
  env:
    COSIGN_EXPERIMENTAL: "true"  # Enable keyless
  run: |
    cosign sign --yes ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-engine-${{ matrix.engine }}:$SHA
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
  aquasec/trivy image moto-dev:latest
```

**Local scanning:**

```bash
# Scan with severity filter
trivy image --severity HIGH,CRITICAL moto-engine-club:latest

# Scan and fail on findings (for CI)
trivy image --exit-code 1 --severity CRITICAL moto-engine-club:latest

# Output as JSON for processing
trivy image --format json --output results.json moto-engine-club:latest

# Scan before pushing (recommended flow)
nix build .#engine-club && docker load < result
trivy image --exit-code 1 --severity HIGH,CRITICAL moto-engine-club:latest
docker push localhost:5000/moto-engine-club:latest
```

**CI workflow addition:**

```yaml
- name: Scan image
  run: |
    # Install trivy
    curl -sfL https://raw.githubusercontent.com/aquasecurity/trivy/main/contrib/install.sh | sh -s -- -b /usr/local/bin

    # Scan - fail on CRITICAL, warn on HIGH
    trivy image --exit-code 1 --severity CRITICAL moto-engine-${{ matrix.engine }}:latest
    trivy image --severity HIGH moto-engine-${{ matrix.engine }}:latest
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
trivy image --format spdx-json --output sbom.spdx.json moto-engine-club:latest

# CycloneDX format (good for security tools)
trivy image --format cyclonedx --output sbom.cdx.json moto-engine-club:latest
```

**Alternative: syft (from Anchore):**

```bash
# Install
brew install syft

# Generate SBOM
syft moto-engine-club:latest -o spdx-json > sbom.spdx.json
```

**Attach SBOM to image as attestation:**

```bash
# Sign and attach SBOM
cosign attest --predicate sbom.spdx.json --type spdx \
  localhost:5000/moto-engine-club:latest

# Verify attestation exists
cosign verify-attestation --type spdx \
  localhost:5000/moto-engine-club:latest
```

**CI workflow addition:**

```yaml
- name: Generate and attach SBOM
  run: |
    # Generate SBOM
    trivy image --format spdx-json --output sbom.spdx.json \
      moto-engine-${{ matrix.engine }}:latest

    # Attach as attestation (if signing enabled)
    cosign attest --yes --predicate sbom.spdx.json --type spdx \
      ${{ env.REGISTRY }}/${{ env.REGISTRY_ORG }}/moto-engine-${{ matrix.engine }}:$SHA
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

**Building locally:**

```bash
# Build garage container
nix build .#garage
docker load < result

# Build specific engine
nix build .#engine-club
docker load < result

# Run garage locally
docker run -it --rm \
  -v $(pwd):/workspace \
  moto-dev:latest
```

**Makefile targets:**

```makefile
REGISTRY ?= localhost:5000
SHA := $(shell git rev-parse --short HEAD)

.PHONY: help docker-build docker-push docker-clean docker-scan

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  %-20s %s\n", $$1, $$2}'

# === Build ===
docker-build: docker-build-garage docker-build-engines ## Build all containers

docker-build-garage: ## Build garage (dev) container
	nix build .#garage
	docker load < result

docker-build-engines: ## Build all engine containers
	nix build .#engine-club && docker load < result
	nix build .#engine-vault && docker load < result

# === Push ===
docker-push: docker-push-garage docker-push-engines ## Push all to registry

docker-push-garage: ## Push garage to registry
	docker tag moto-dev:latest $(REGISTRY)/moto-dev:latest
	docker tag moto-dev:latest $(REGISTRY)/moto-dev:$(SHA)
	docker push $(REGISTRY)/moto-dev:latest
	docker push $(REGISTRY)/moto-dev:$(SHA)

docker-push-engines: ## Push engines to registry
	docker tag moto-engine-club:latest $(REGISTRY)/moto-engine-club:latest
	docker tag moto-engine-vault:latest $(REGISTRY)/moto-engine-vault:latest
	docker push $(REGISTRY)/moto-engine-club:latest
	docker push $(REGISTRY)/moto-engine-vault:latest

# === Run ===
docker-run-garage: ## Run garage container with workspace mounted
	docker run -it --rm -v $(PWD):/workspace moto-dev:latest

# === Inspect & Debug ===
docker-inspect: ## Show image layers and sizes
	@echo "=== moto-dev ===" && docker history moto-dev:latest
	@echo "=== moto-engine-club ===" && docker history moto-engine-club:latest 2>/dev/null || true

docker-scan: ## Scan images for vulnerabilities (requires trivy)
	trivy image --severity HIGH,CRITICAL moto-dev:latest
	trivy image --severity HIGH,CRITICAL moto-engine-club:latest

# === Cleanup ===
docker-clean: ## Remove all moto images
	docker images --filter=reference='moto-*' -q | xargs -r docker rmi -f
	docker images --filter=reference='*/moto-*' -q | xargs -r docker rmi -f

# === Registry ===
registry-start: ## Start local registry
	docker run -d -p 5000:5000 --name moto-registry registry:2 || true

registry-stop: ## Stop local registry
	docker stop moto-registry && docker rm moto-registry || true
```

### Image Size Targets

| Image | Target | Rationale |
|-------|--------|-----------|
| `moto-dev` | < 3GB | Full toolchain acceptable for dev |
| `moto-engine-*` | < 50MB | Minimal for fast deploys |

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
| `bike.md` | Defines bike container requirements |
| `moto-club.md` | Club runs in bike container |
| `garage-lifecycle.md` | Manages garage container instances |
| `k3s-cluster.md` | Where containers run |

### Troubleshooting

**Build failures:**

```bash
# View detailed Nix build logs
nix build .#garage -L

# Keep failed build for inspection
nix build .#garage --keep-failed

# Check what's in a derivation
nix path-info -rsh .#garage
```

**Image inspection:**

```bash
# View layers and sizes
docker history moto-dev:latest

# Deep dive with dive tool
dive moto-dev:latest

# Export and inspect filesystem
docker save moto-dev:latest | tar -tvf -

# Compare two image versions
docker history moto-dev:v1 > v1.txt
docker history moto-dev:v2 > v2.txt
diff v1.txt v2.txt
```

**Debugging bike containers (no shell):**

Bike containers have no shell. Use ephemeral debug containers:

```bash
# Attach debug container to running pod
kubectl debug -it moto-club-abc123 --image=busybox --target=club

# Or run the image with shell override (for local testing)
# This won't work because there's no shell, but you can:
docker run --rm -it --entrypoint="" moto-engine-club:latest /bin/sh
# Error: executable file not found

# Instead, use a sidecar approach or copy binary out:
docker create --name temp moto-engine-club:latest
docker cp temp:/bin/moto-engine-club ./moto-engine-club
docker rm temp
# Now inspect the binary locally
```

**Registry issues:**

```bash
# Test registry connectivity
curl -s http://localhost:5000/v2/_catalog

# Check if image exists
curl -s http://localhost:5000/v2/moto-dev/tags/list

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
nix build .#engine-club -o result1
nix build .#engine-club -o result2
diff <(sha256sum result1) <(sha256sum result2)
# Should be identical

# Check derivation hash
nix path-info --json .#engine-club | jq '.[] | .path'
```

## Notes

- Consider distroless base for bike containers (though Nix already builds minimal images)
- `cargo audit` and `cargo deny` are included in the flake's `checks` output
- Run `nix flake check` in CI for Nix validation (clippy, fmt, audit)

## References

- [Nix dockerTools](https://nixos.org/manual/nixpkgs/stable/#sec-pkgs-dockerTools)
- [Loom container system](https://github.com/ghuntley/loom) - reference architecture
- [OCI Image Spec](https://github.com/opencontainers/image-spec)
