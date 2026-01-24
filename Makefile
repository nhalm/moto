.PHONY: build test check fmt lint clean run fix ci docker-build-moto-garage docker-test-moto-garage docker-shell-moto-garage

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

# Build the moto-garage container image using Nix
docker-build-moto-garage:
	@echo "Building moto-garage container..."
	nix build .#moto-garage --print-out-paths | xargs docker load <

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
