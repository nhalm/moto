.PHONY: install build test check fmt lint clean run fix ci
.PHONY: test-ci
.PHONY: build-garage test-garage shell-garage push-garage scan-garage clean-images clean-nix-cache
.PHONY: build-bike test-bike
.PHONY: build-club push-club build-keybox push-keybox
.PHONY: registry-start registry-stop
.PHONY: test-db-up test-db-down test-db-migrate test-integration test-all
.PHONY: dev dev-cluster dev-up dev-down dev-clean
.PHONY: dev-db-up dev-db-down dev-db-migrate dev-keybox-init dev-keybox dev-club dev-garage-image

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

# === Container (Service Images) ===

# Build the moto-club container image using Docker-wrapped Nix
build-club:
	@echo "Building moto-club container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-club-image --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

# Build the moto-keybox container image using Docker-wrapped Nix
build-keybox:
	@echo "Building moto-keybox container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-keybox-image --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

# === Push ===

# Default registry for pushing images
REGISTRY ?= localhost:5050
SHA := $(shell git rev-parse --short HEAD)

# Push moto-garage to registry, clean up local copies (saves ~10GB VM disk)
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
	@echo "Cleaning up local Docker copies..."
	-docker rmi moto-garage:latest $(REGISTRY)/moto-garage:latest $(REGISTRY)/moto-garage:$(SHA) 2>/dev/null
	@echo "Local copies removed (image lives in registry)."

# Push moto-club to registry
push-club:
	@if ! docker image inspect moto-club:latest &>/dev/null; then \
		echo "Error: moto-club:latest not found. Run 'make build-club' first."; \
		exit 1; \
	fi
	@echo "Pushing moto-club to $(REGISTRY)..."
	docker tag moto-club:latest $(REGISTRY)/moto-club:latest
	docker tag moto-club:latest $(REGISTRY)/moto-club:$(SHA)
	docker push $(REGISTRY)/moto-club:latest
	docker push $(REGISTRY)/moto-club:$(SHA)
	@echo "Pushed $(REGISTRY)/moto-club:latest and $(REGISTRY)/moto-club:$(SHA)"

# Push moto-keybox to registry
push-keybox:
	@if ! docker image inspect moto-keybox:latest &>/dev/null; then \
		echo "Error: moto-keybox:latest not found. Run 'make build-keybox' first."; \
		exit 1; \
	fi
	@echo "Pushing moto-keybox to $(REGISTRY)..."
	docker tag moto-keybox:latest $(REGISTRY)/moto-keybox:latest
	docker tag moto-keybox:latest $(REGISTRY)/moto-keybox:$(SHA)
	docker push $(REGISTRY)/moto-keybox:latest
	docker push $(REGISTRY)/moto-keybox:$(SHA)
	@echo "Pushed $(REGISTRY)/moto-keybox:latest and $(REGISTRY)/moto-keybox:$(SHA)"

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

# Fresh database cycle: teardown, start, migrate, run integration tests, teardown
test-integration: test-db-down test-db-up test-db-migrate
	TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test --features integration; \
	status=$$?; \
	$(MAKE) test-db-down; \
	exit $$status

# Run unit tests + integration tests (fresh database cycle)
test-all: test
	$(MAKE) test-integration

# === Local Development ===

# Dev database URL for moto-club
DEV_DATABASE_URL ?= postgres://moto:moto@localhost:5432/moto_club

# Alias for moto dev up
dev:
	cargo run --bin moto -- dev up

# Create k3d cluster via moto CLI (idempotent)
dev-cluster:
	cargo run --bin moto -- cluster init

# Start full local dev stack (postgres + keybox + club in foreground)
# Runs setup steps, then starts keybox in background and moto-club in foreground
# Ctrl-C stops everything
dev-up: dev-db-up dev-keybox-init dev-db-migrate
	@echo "Starting keybox in background..."
	@MOTO_KEYBOX_BIND_ADDR=0.0.0.0:8090 \
	MOTO_KEYBOX_HEALTH_BIND_ADDR=0.0.0.0:8091 \
	MOTO_KEYBOX_MASTER_KEY_FILE=.dev/keybox/master.key \
	MOTO_KEYBOX_SVID_SIGNING_KEY_FILE=.dev/keybox/signing.key \
	MOTO_KEYBOX_DATABASE_URL=postgres://moto:moto@localhost:5432/moto_keybox \
	MOTO_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token \
	RUST_LOG=moto_keybox=debug \
	cargo run --bin moto-keybox-server & \
	KEYBOX_PID=$$!; \
	trap "kill $$KEYBOX_PID 2>/dev/null; wait $$KEYBOX_PID 2>/dev/null" EXIT INT TERM; \
	echo "Keybox started (PID $$KEYBOX_PID)"; \
	echo "Starting moto-club in foreground (Ctrl-C to stop all)..."; \
	MOTO_CLUB_DATABASE_URL=postgres://moto:moto@localhost:5432/moto_club \
	MOTO_CLUB_KEYBOX_URL=http://localhost:8090 \
	MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token \
	MOTO_CLUB_KEYBOX_HEALTH_URL=http://localhost:8091 \
	MOTO_CLUB_DEV_CONTAINER_IMAGE=moto-registry:5000/moto-garage:latest \
	RUST_LOG=moto_club=debug \
	cargo run --bin moto-club

# Start dev database via docker-compose, wait for healthcheck
dev-db-up:
	@echo "Starting dev database..."
	docker compose up -d --wait
	@echo "Dev database ready on port 5432."

# Stop dev database
dev-db-down:
	@echo "Stopping dev database..."
	docker compose down
	@echo "Dev database stopped."

# Run moto-club-db migrations against dev database
dev-db-migrate:
	@echo "Running moto-club-db migrations..."
	cargo sqlx migrate run --source crates/moto-club-db/migrations --database-url $(DEV_DATABASE_URL)
	@echo "Migrations complete."

# Generate keybox keys in .dev/keybox/ (master.key, signing.key, service-token)
# Idempotent: skips if all keys already exist. To regenerate: rm -rf .dev/keybox && make dev-keybox-init
dev-keybox-init:
	@if [ -f .dev/keybox/master.key ] && [ -f .dev/keybox/signing.key ] && [ -f .dev/keybox/service-token ]; then \
		echo "Keybox keys already exist in .dev/keybox/"; \
	else \
		mkdir -p .dev/keybox && \
		cargo run --bin moto-keybox -- init --output-dir .dev/keybox --force && \
		openssl rand -hex 32 > .dev/keybox/service-token && \
		chmod 600 .dev/keybox/service-token && \
		echo "Keybox keys generated in .dev/keybox/"; \
	fi

# Start moto-keybox-server with dev config (runs in foreground)
dev-keybox:
	MOTO_KEYBOX_BIND_ADDR=0.0.0.0:8090 \
	MOTO_KEYBOX_HEALTH_BIND_ADDR=0.0.0.0:8091 \
	MOTO_KEYBOX_MASTER_KEY_FILE=.dev/keybox/master.key \
	MOTO_KEYBOX_SVID_SIGNING_KEY_FILE=.dev/keybox/signing.key \
	MOTO_KEYBOX_DATABASE_URL=postgres://moto:moto@localhost:5432/moto_keybox \
	MOTO_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token \
	RUST_LOG=moto_keybox=debug \
	cargo run --bin moto-keybox-server

# Build and push garage image to local registry
dev-garage-image: build-garage push-garage

# Start moto-club with dev config (runs in foreground)
dev-club:
	MOTO_CLUB_DATABASE_URL=postgres://moto:moto@localhost:5432/moto_club \
	MOTO_CLUB_KEYBOX_URL=http://localhost:8090 \
	MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token \
	MOTO_CLUB_KEYBOX_HEALTH_URL=http://localhost:8091 \
	MOTO_CLUB_DEV_CONTAINER_IMAGE=moto-registry:5000/moto-garage:latest \
	RUST_LOG=moto_club=debug \
	cargo run --bin moto-club

# Stop all dev services and database
dev-down:
	@echo "Stopping dev services..."
	docker compose down
	@echo "Dev services stopped."

# Stop all dev services, remove database volume, and remove dev state
dev-clean:
	@echo "Cleaning dev state..."
	docker compose down -v
	rm -rf .dev/
	@echo "Dev state cleaned."
