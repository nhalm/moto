# Makefile

| | |
|--------|----------------------------------------------|
| Version | 0.6 |
| Status | Ready to Rip |
| Last Updated | 2026-02-04 |

## Overview

Defines the Makefile structure and targets for the moto project. The Makefile is the primary interface for development tasks.

## Specification

### Prerequisites

The following tools should be installed before running `make install`:

| Tool | Purpose | Installation |
|------|---------|--------------|
| **Nix** | Package manager for reproducible devShell | See below |
| **Docker** | Container runtime | Docker Desktop, Colima, or OrbStack |
| **Git** | Version control | `brew install git` or system package |

**Nix Installation (Determinate installer recommended):**

```bash
curl --proto '=https' --tlsv1.2 -sSf -L https://install.determinate.systems/nix | sh -s -- install
```

After installing, open a new terminal or run:
```bash
. /nix/var/nix/profiles/default/etc/profile.d/nix-daemon.sh
```

Verify with: `nix --version`

**Why Determinate Nix:**
- Survives macOS upgrades
- Enables flakes by default
- Generates uninstall receipt for clean removal

### Target Groups

| Group | Purpose |
|-------|---------|
| **Setup** | Local development environment setup |
| **Development** | Build, test, lint, format |
| **Container** | Build and test container images |
| **Cluster** | Local k3s operations |

### Setup Targets

```makefile
# Set up local development environment (run once)
install:
	git config core.hooksPath .githooks
	# Future: k3s setup, other dependencies
```

The `install` target is idempotent - safe to run multiple times.

### Development Targets

```makefile
build:          # Build all crates
test:           # Run all tests
check:          # Check compilation (no build)
fmt:            # Format code
lint:           # Run clippy
clean:          # Clean build artifacts
fix:            # Auto-fix lint issues
ci:             # Full CI check (fmt + check + lint + test)
```

### Container Targets

```makefile
# Build
build-garage:               # Build garage container (Docker-wrapped Nix, works on Mac and Linux)

# Test and debug
test-garage:                # Run smoke tests on container
shell-garage:               # Interactive shell in container

# Push
push-garage:                # Push garage image to local registry (localhost:5000)

# Maintenance
scan-garage:                # Scan image for vulnerabilities (requires trivy)
clean-images:               # Remove all moto images
clean-nix-cache:            # Remove Docker volume used for Nix store caching
```

**How `build-garage` works:**

1. Runs a `nixos/nix` container with the repo mounted
2. Executes `nix build .#moto-garage` inside the container
3. Loads the resulting image via `docker load`
4. Pushes to local registry

This approach uses the Nix flake as the single source of truth while working on any platform (Mac or Linux). Architecture is auto-detected - ARM Mac builds `aarch64-linux`, Intel builds `x86_64-linux`.

**CI builds differently:** GitHub Actions installs Nix directly on Linux runners and runs `nix build` without Docker. See [container-system.md](container-system.md) for CI workflow.

**Nix cache:** The Docker-wrapped build uses a named volume (`nix-store`) to cache the Nix store between builds. This speeds up subsequent builds but can grow over time. Use `clean-nix-cache` to remove it and force a fresh build.

**Error handling:** If `build-garage` fails:
- Verify Docker is running (`docker info`)
- Check the `nixos/nix` image is available (`docker pull nixos/nix:latest`)
- For detailed output, the target should print the nix build command on failure

### Registry Targets

```makefile
registry-start:    # Start local Docker registry on localhost:5000
registry-stop:     # Stop and remove local registry
```

Optional targets for local development with a registry.

### Cluster Targets (Future)

```makefile
k3s-install:    # Install k3s locally
k3s-start:      # Start local cluster
k3s-stop:       # Stop local cluster
k3s-status:     # Show cluster status
```

### Naming Conventions

- Use hyphens, not underscores: `build-garage` not `build_garage`
- Pattern: `<action>-<target>` (e.g., `build-garage`, `test-garage`, `push-garage`)
- Keep names short but clear

### Phony Targets

All targets should be declared `.PHONY` since they don't produce files:

```makefile
.PHONY: install build test check fmt lint clean fix ci run
.PHONY: build-garage test-garage shell-garage push-garage scan-garage clean-images clean-nix-cache
.PHONY: build-bike test-bike
.PHONY: registry-start registry-stop
```

## Changelog

### v0.6 (2026-02-04)
- Add `build-bike` and `test-bike` targets (were implemented but not documented)
- Add `run` to phony targets

### v0.5 (2026-01-26)
- Add `clean-nix-cache` target for removing Docker volume used by Nix store
- Add error handling guidance for `build-garage` failures
- Document Nix cache behavior

### v0.4 (2026-01-26)
- New container targets: `build-garage`, `test-garage`, `shell-garage`, `push-garage`, `scan-garage`, `clean-images`
- `build-garage` uses Docker-wrapped Nix: runs `nix build` inside `nixos/nix` container
- Works on Mac without configuring a Linux builder
- CI uses direct `nix build` on Linux runners (not Makefile)
- Remove old `docker-build-moto-garage` naming convention

### v0.3 (2026-01-26)
- Correct spec to match implementation: Nix dockerTools, not Dockerfile
- Document push, scan, clean, registry targets
- Remove multi-arch target (handled by flake outputs)
- Mark as Ripping (implementation complete)

### v0.2 (2026-01-25)
- Add Prerequisites section with Nix installation instructions
- (Spec update that diverged from implementation - corrected in v0.3)

### v0.1 (2026-01-24)
- Initial specification

## References

- [pre-commit.md](pre-commit.md) - Hook setup via `make install`
- [dev-container.md](dev-container.md) - Container build targets
