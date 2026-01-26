# Makefile

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Ready to Rip |
| Last Updated | 2026-01-24 |

## Overview

Defines the Makefile structure and targets for the moto project. The Makefile is the primary interface for development tasks.

## Specification

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
docker-build-moto-garage:   # Build garage container via Nix
docker-test-moto-garage:    # Run smoke tests on container
docker-shell-moto-garage:   # Interactive shell in container
```

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
```

## References

- [pre-commit.md](pre-commit.md) - Hook setup via `make install`
- [dev-container.md](dev-container.md) - Container build targets
