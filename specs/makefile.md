# Makefile

| | |
|--------|----------------------------------------------|
| Version | 0.14 |
| Status | Ready to Rip |
| Last Updated | 2026-02-26 |

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
| **Testing** | Test database lifecycle and integration tests |
| **Local Dev** | Run the full stack locally |
| **Deploy** | Deploy services to K8s cluster |

### Default Target

Running `make` with no arguments prints all available targets grouped by category. `make help` does the same thing.

```makefile
.DEFAULT_GOAL := help

help:            # Show all available targets
```

The output should list every target with its comment, grouped by section headers (Setup, Development, Container, etc.).

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
# Garage (dev container)
build-garage:               # Build garage container (Docker-wrapped Nix, works on Mac and Linux)
test-garage:                # Run smoke tests on container
shell-garage:               # Interactive shell in container
push-garage:                # Push garage image to local registry (localhost:5000)

# Service images (bike base + binary)
build-club:                 # Build moto-club container image
push-club:                  # Push moto-club to local registry, clean up local copy
build-keybox:               # Build moto-keybox container image
push-keybox:                # Push moto-keybox to local registry, clean up local copy

# Maintenance
scan-garage:                # Scan image for vulnerabilities (requires trivy)
clean-images:               # Remove all moto images
clean-nix-cache:            # Remove Docker volume used for Nix store caching
```

**How container builds work:**

All `build-*` targets use Docker-wrapped Nix: they run `nix build` inside a `nixos/nix` container with the repo mounted, then load the resulting image via `docker load`. This keeps the Nix flake as the single source of truth while working on any platform — ARM Mac builds `aarch64-linux`, Intel builds `x86_64-linux`.

A named Docker volume (`nix-store`) caches the Nix store between builds. Use `clean-nix-cache` to remove it and force a fresh build.

**CI builds differently:** GitHub Actions installs Nix directly on Linux runners and runs `nix build` without Docker. See [container-system.md](container-system.md) for CI workflow.

**If `build-*` targets fail:** verify Docker is running, check that the `nixos/nix` image is available, and look for detailed output from the nix build command.

### Registry Targets

```makefile
registry-start:    # Start local Docker registry on localhost:5000
registry-stop:     # Stop and remove local registry
```

Optional targets for local development with a registry.

### Testing Targets

```makefile
test-db-up:          # Start test database via docker-compose.test.yml, wait for healthcheck
test-db-down:        # Stop test database, remove volumes
test-db-migrate:     # Run migrations for moto-club-db AND moto-keybox-db against test database
test-integration:    # Fresh database cycle: test-db-up + test-db-migrate + integration tests + test-db-down
test-all:            # Every test: unit + integration + ignored (K8s) — no test left behind
test-ci:             # For CI: assumes database already running, runs unit + integration tests
```

`test-all` runs every test in the project:

- Unit tests (`cargo test --lib`)
- Integration tests (with fresh test database)
- Ignored tests (`#[ignore]` — e.g., tests requiring a running K8s cluster)

Each category runs once (no duplicate runs). Tests that require root are excluded.

See [testing.md](testing.md) for test infrastructure specification.

### Local Dev Targets

```makefile
dev-cluster:         # Create k3d cluster via moto CLI (idempotent)
dev-cluster-down:    # Delete the k3d cluster and local registry
dev-up:              # Start full local dev stack (postgres + keybox + club)
dev-down:            # Stop all services and database
dev-clean:           # dev-down + remove pgdata volume + remove .dev/
dev-db-up:           # Start dev postgres (docker-compose.yml, port 5432)
dev-db-down:         # Stop dev postgres
dev-db-migrate:      # Run moto-club-db migrations against dev database
dev-keybox-init:     # Generate keybox keys in .dev/keybox/
dev-keybox:          # Start moto-keybox-server with dev config
dev-club:            # Start moto-club with dev config
dev-garage-image:    # Build and push garage image to local registry
```

See [local-dev.md](local-dev.md) for full local development specification.

### Deploy Targets

```makefile
deploy-images:       # Build and push all service images (garage, club, keybox)
deploy-secrets:      # Generate and apply K8s secrets to moto-system namespace
deploy-system:       # Deploy all moto-system components (kubectl apply -k)
deploy-status:       # Show status of moto-system pods
deploy:              # Full deploy: deploy-images + deploy-secrets + deploy-system + deploy-status
```

`deploy-images` builds and pushes all three images (`moto-garage`, `moto-club`, `moto-keybox`) to the local registry. It is a prerequisite for `deploy-system` — without it, pods will enter `ImagePullBackOff`.

`deploy` is the one-command deployment path: it runs the full sequence from images through status verification. Equivalent to the manual Deployment Flow in [service-deploy.md](service-deploy.md) (steps 2-5).

See [service-deploy.md](service-deploy.md) for K8s deployment specification.

### Naming Conventions

- Use hyphens, not underscores: `build-garage` not `build_garage`
- Pattern: `<action>-<target>` (e.g., `build-garage`, `test-garage`, `push-garage`)
- Keep names short but clear

## Changelog

### v0.14 (2026-02-26)
- Add `dev-cluster-down` target to delete the k3d cluster and local registry

### v0.13 (2026-02-26)
- `test-all` runs every test category: unit, integration, and `#[ignore]` tests (K8s)
- Each category runs exactly once (no duplicate unit test runs)
- Tests requiring root remain excluded

### v0.12 (2026-02-26)
- Add `help` as default target: `make` with no arguments prints all available targets grouped by category
- `.DEFAULT_GOAL := help`

### v0.11 (2026-02-26)
- `push-club` and `push-keybox` clean up local Docker images after pushing (same as `push-garage`; saves disk space since images only need to live in the registry)

### v0.10 (2026-02-25)
- Add `deploy-images` target: builds and pushes all three service images (garage, club, keybox) to local registry
- Add `deploy` target: full deployment flow (deploy-images + deploy-secrets + deploy-system + deploy-status)

### v0.9 (2026-02-20)
- Add `dev-cluster` target for k3d cluster creation via moto CLI

### v0.8 (2026-02-19)
- Add `test-ci` to testing targets (was implemented but not in spec)

### v0.7 (2026-02-18)
- Add service container targets: `build-club`, `push-club`, `build-keybox`, `push-keybox`
- Add testing targets: `test-db-up`, `test-db-down`, `test-db-migrate`, `test-integration`, `test-all`
- Add local dev targets: `dev-up`, `dev-down`, `dev-clean`, `dev-db-up`, `dev-db-down`, `dev-db-migrate`, `dev-keybox-init`, `dev-keybox`, `dev-club`, `dev-garage-image`
- Add deploy targets: `deploy-secrets`, `deploy-system`, `deploy-status`, `undeploy-system`
- Replace "Cluster Targets (Future)" section with Testing, Local Dev, and Deploy sections

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
- [testing.md](testing.md) - Test infrastructure and database targets
- [local-dev.md](local-dev.md) - Local development workflow
- [service-deploy.md](service-deploy.md) - K8s deployment
- [container-system.md](container-system.md) - Image build pipeline
