.PHONY: install build test check fmt lint clean run fix ci
.PHONY: test-ci
.PHONY: build-garage test-garage shell-garage push-garage scan-garage clean-images clean-nix-cache
.PHONY: build-bike test-bike
.PHONY: registry-start registry-stop
.PHONY: test-db-up test-db-down test-db-migrate test-integration test-all

# Set up local development environment
install:
	git config core.hooksPath .githooks

# Build all crates
build:
	cargo build --workspace

# Test database URL (override with environment variable if needed)
TEST_DATABASE_URL ?= postgres://moto_test:moto_test@localhost:5433/moto_test

# Run unit tests only (fast, no dependencies)
test:
	cargo test --lib

# Check compilation without building
check:
	cargo check --workspace

# Format code
fmt:
	cargo fmt --all

# Run clippy lints
lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Clean build artifacts
clean:
	cargo clean

# Run the CLI (when implemented)
run:
	cargo run --bin moto-cli

# Format and lint
fix: fmt
	cargo clippy --workspace --all-targets --fix --allow-dirty

# Full CI check
ci: fmt check lint test

# CI target (assumes database is already running)
test-ci:
	cargo test --lib
	TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test --features integration

# === Container (Garage) ===

# Detect Linux target based on host architecture
# aarch64-darwin -> aarch64-linux, x86_64-darwin -> x86_64-linux
NIX_LINUX_SYSTEM := $(shell uname -m | sed 's/arm64/aarch64/')-linux

# Build the moto-garage container image using Docker-wrapped Nix
# This runs nix build inside a nixos/nix container, works on Mac without configuring a Linux builder
build-garage:
	@echo "Building moto-garage container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-garage --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

# Build and run smoke tests on the container
test-garage: build-garage
	@echo "Running smoke tests..."
	./infra/smoke-test.sh

# Interactive shell in the container for debugging
shell-garage:
	@if ! docker image inspect moto-garage:latest &>/dev/null; then \
		echo "Image not found, building..."; \
		$(MAKE) build-garage; \
	fi
	docker run -it --rm moto-garage:latest

# === Container (Bike) ===

# Build the moto-bike base container image using Docker-wrapped Nix
# This is the minimal production image (CA certs, tzdata, non-root user only)
build-bike:
	@echo "Building moto-bike container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-bike --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

# Build and run smoke tests on the bike container
test-bike: build-bike
	@echo "Running bike smoke tests..."
	./infra/smoke-test-bike.sh

# === Push ===

# Default registry for pushing images
REGISTRY ?= localhost:5000
SHA := $(shell git rev-parse --short HEAD)

# Push moto-garage to registry (localhost:5000 by default)
push-garage:
	@if ! docker image inspect moto-garage:latest &>/dev/null; then \
		echo "Error: moto-garage:latest not found. Run 'make build-garage' first."; \
		exit 1; \
	fi
	@echo "Pushing moto-garage to $(REGISTRY)..."
	docker tag moto-garage:latest $(REGISTRY)/moto-garage:latest
	docker tag moto-garage:latest $(REGISTRY)/moto-garage:$(SHA)
	docker push $(REGISTRY)/moto-garage:latest
	docker push $(REGISTRY)/moto-garage:$(SHA)
	@echo "Pushed $(REGISTRY)/moto-garage:latest and $(REGISTRY)/moto-garage:$(SHA)"

# === Scan ===

# Scan images for vulnerabilities using trivy
scan-garage:
	@if ! command -v trivy &>/dev/null; then \
		echo "Error: trivy is not installed. Install with 'brew install trivy' or 'nix-shell -p trivy'"; \
		exit 1; \
	fi
	@echo "Scanning moto-garage for vulnerabilities..."
	@if ! docker image inspect moto-garage:latest &>/dev/null; then \
		echo "Error: moto-garage:latest not found. Run 'make build-garage' first."; \
		exit 1; \
	fi
	trivy image --severity HIGH,CRITICAL moto-garage:latest

# === Cleanup ===

# Remove all moto container images
clean-images:
	@echo "Removing moto images..."
	-docker images --filter=reference='moto-*' -q | xargs docker rmi -f 2>/dev/null || true
	-docker images --filter=reference='*/moto-*' -q | xargs docker rmi -f 2>/dev/null || true
	@echo "Done."

# Remove Docker volume used for Nix store caching
# This forces a fresh build but speeds up subsequent builds after running
clean-nix-cache:
	@echo "Removing Nix store cache volume..."
	-docker volume rm nix-store 2>/dev/null && echo "Removed nix-store volume." || \
		echo "nix-store volume does not exist or is in use."

# === Registry ===

# Start local registry for development
registry-start:
	@echo "Starting local registry on localhost:5000..."
	@docker run -d -p 5000:5000 --name moto-registry registry:2 2>/dev/null || \
		(docker start moto-registry 2>/dev/null && echo "Registry already exists, started.") || \
		echo "Registry already running."

# Stop local registry
registry-stop:
	@echo "Stopping local registry..."
	@docker stop moto-registry 2>/dev/null && docker rm moto-registry 2>/dev/null && echo "Registry stopped and removed." || \
		echo "Registry not running or already removed."

# === Testing ===

# Start test database via docker-compose, wait for healthcheck
test-db-up:
	@echo "Starting test database..."
	docker compose -f docker-compose.test.yml up -d --wait
	@echo "Test database ready on port 5433."

# Stop test database and remove volumes
test-db-down:
	@echo "Stopping test database..."
	docker compose -f docker-compose.test.yml down -v
	@echo "Test database stopped."

# Run migrations for all database crates against test database
# --ignore-missing: both crates share one database, so each sees the other's migrations as "missing"
test-db-migrate:
	@echo "Running moto-club-db migrations..."
	cargo sqlx migrate run --source crates/moto-club-db/migrations --database-url $(TEST_DATABASE_URL) --ignore-missing
	@echo "Running moto-keybox-db migrations..."
	cargo sqlx migrate run --source crates/moto-keybox-db/migrations --database-url $(TEST_DATABASE_URL) --ignore-missing
	@echo "All migrations complete."
