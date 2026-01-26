# Makefile

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Status | Ripping |
| Last Updated | 2026-01-26 |

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
docker-build-moto-garage:   # Build garage container via Nix (auto-detects arch)
docker-test-moto-garage:    # Run smoke tests on container
docker-shell-moto-garage:   # Interactive shell in container
docker-push-moto-garage:    # Push garage image to registry
docker-push-local:          # Push all images to localhost:5000
docker-scan:                # Scan images for vulnerabilities (requires trivy)
docker-clean:               # Remove all moto images
```

The garage container uses Nix `dockerTools.buildLayeredImage`. The Makefile auto-detects architecture (ARM vs Intel) and builds the appropriate Linux target.

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

- Use hyphens, not underscores: `docker-build` not `docker_build`
- Group prefix for related targets: `docker-*`, `k3s-*`
- Keep names short but clear

### Phony Targets

All targets should be declared `.PHONY` since they don't produce files:

```makefile
.PHONY: install build test check fmt lint clean fix ci
.PHONY: docker-build-moto-garage docker-test-moto-garage docker-shell-moto-garage
.PHONY: docker-push-moto-garage docker-push-local docker-scan docker-clean
.PHONY: registry-start registry-stop
```

## Changelog

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
