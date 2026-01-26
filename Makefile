.PHONY: install build test check fmt lint clean run fix ci docker-build-moto-garage docker-test-moto-garage docker-shell-moto-garage docker-push-moto-garage docker-push-local docker-clean registry-start registry-stop

# Set up local development environment
install:
	git config core.hooksPath .githooks

# Build all crates
build:
	cargo build --workspace

# Run all tests
test:
	cargo test --workspace

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

# === Dev Container (Garage) ===

# Detect Linux target based on host architecture
# aarch64-darwin -> aarch64-linux, x86_64-darwin -> x86_64-linux
NIX_LINUX_SYSTEM := $(shell uname -m | sed 's/arm64/aarch64/')-linux

# Build the moto-garage container image using Nix
docker-build-moto-garage:
	@echo "Building moto-garage container for $(NIX_LINUX_SYSTEM)..."
	docker load < $$(nix build .#packages.$(NIX_LINUX_SYSTEM).moto-garage --print-out-paths)

# Build and run smoke tests on the container
docker-test-moto-garage: docker-build-moto-garage
	@echo "Running smoke tests..."
	./infra/smoke-test.sh

# Interactive shell in the container for debugging
docker-shell-moto-garage:
	@if ! docker image inspect moto-garage:latest &>/dev/null; then \
		echo "Image not found, building..."; \
		$(MAKE) docker-build-moto-garage; \
	fi
	docker run -it --rm moto-garage:latest

# === Push ===

# Default registry for pushing images
REGISTRY ?= localhost:5000
SHA := $(shell git rev-parse --short HEAD)

# Push all images to local registry (localhost:5000)
# Currently only pushes moto-garage; moto-engine will be added when bike.md is ready
docker-push-local: docker-build-moto-garage
	@echo "Pushing all images to $(REGISTRY)..."
	docker tag moto-garage:latest $(REGISTRY)/moto-garage:latest
	docker tag moto-garage:latest $(REGISTRY)/moto-garage:$(SHA)
	docker push $(REGISTRY)/moto-garage:latest
	docker push $(REGISTRY)/moto-garage:$(SHA)
	@echo "Pushed $(REGISTRY)/moto-garage:latest and $(REGISTRY)/moto-garage:$(SHA)"

# Push moto-garage to registry
docker-push-moto-garage:
	@if ! docker image inspect moto-garage:latest &>/dev/null; then \
		echo "Error: moto-garage:latest not found. Run 'make docker-build-moto-garage' first."; \
		exit 1; \
	fi
	@echo "Pushing moto-garage to $(REGISTRY)..."
	docker tag moto-garage:latest $(REGISTRY)/moto-garage:latest
	docker tag moto-garage:latest $(REGISTRY)/moto-garage:$(SHA)
	docker push $(REGISTRY)/moto-garage:latest
	docker push $(REGISTRY)/moto-garage:$(SHA)
	@echo "Pushed $(REGISTRY)/moto-garage:latest and $(REGISTRY)/moto-garage:$(SHA)"

# === Cleanup ===

# Remove all moto container images
docker-clean:
	@echo "Removing moto images..."
	-docker images --filter=reference='moto-*' -q | xargs docker rmi -f 2>/dev/null || true
	-docker images --filter=reference='*/moto-*' -q | xargs docker rmi -f 2>/dev/null || true
	@echo "Done."

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
