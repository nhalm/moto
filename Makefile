.DEFAULT_GOAL := help

.PHONY: help install build test check fmt lint clean run fix ci
.PHONY: test-ci
.PHONY: build-garage test-garage shell-garage push-garage scan-garage clean-images clean-nix-cache
.PHONY: build-bike test-bike
.PHONY: build-club push-club build-keybox push-keybox
.PHONY: registry-start registry-stop
.PHONY: test-db-up test-db-down test-db-migrate test-integration test-all
.PHONY: dev dev-cluster dev-cluster-down dev-up dev-down dev-clean
.PHONY: dev-db-up dev-db-down dev-db-migrate dev-keybox-init dev-keybox dev-club dev-garage-image
.PHONY: deploy-images deploy-secrets deploy-system deploy-status deploy

help: ## Show all available targets
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make \033[36m<target>\033[0m\n"} /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } /^[a-zA-Z_-]+:.*?## / { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

##@ Setup

install: ## Set up local development environment
	git config core.hooksPath .githooks

##@ Development

build: ## Build all crates
	cargo build --workspace

# Test database URL (override with environment variable if needed)
TEST_DATABASE_URL ?= postgres://moto_test:moto_test@localhost:5433/moto_test

test: ## Run unit tests only (fast, no dependencies)
	cargo test --lib

check: ## Check compilation without building
	cargo check --workspace

fmt: ## Format code
	cargo fmt --all

lint: ## Run clippy lints
	cargo clippy --workspace --all-targets -- -D warnings

clean: ## Clean build artifacts
	cargo clean

run: ## Run the CLI
	cargo run --bin moto-cli

fix: fmt ## Auto-fix lint issues
	cargo clippy --workspace --all-targets --fix --allow-dirty

ci: fmt check lint test ## Full CI check (fmt + check + lint + test)

##@ Container

# Detect Linux target based on host architecture
# aarch64-darwin -> aarch64-linux, x86_64-darwin -> x86_64-linux
NIX_LINUX_SYSTEM := $(shell uname -m | sed 's/arm64/aarch64/')-linux

build-garage: ## Build garage container (Docker-wrapped Nix)
	@echo "Building moto-garage container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-garage --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

test-garage: build-garage ## Run smoke tests on garage container
	@echo "Running smoke tests..."
	./infra/smoke-test.sh

shell-garage: ## Interactive shell in garage container
	@if ! docker image inspect moto-garage:latest &>/dev/null; then \
		echo "Image not found, building..."; \
		$(MAKE) build-garage; \
	fi
	docker run -it --rm moto-garage:latest

build-bike: ## Build moto-bike base container
	@echo "Building moto-bike container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-bike --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

test-bike: build-bike ## Run smoke tests on bike container
	@echo "Running bike smoke tests..."
	./infra/smoke-test-bike.sh

build-club: ## Build moto-club container image
	@echo "Building moto-club container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-club-image --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

build-keybox: ## Build moto-keybox container image
	@echo "Building moto-keybox container for $(NIX_LINUX_SYSTEM) via Docker-wrapped Nix..."
	docker run --rm \
		-v $(PWD):/workspace \
		-v nix-store:/nix \
		-w /workspace \
		nixos/nix:latest \
		sh -c "nix build .#packages.$(NIX_LINUX_SYSTEM).moto-keybox-image --extra-experimental-features 'nix-command flakes' -o /tmp/result && cat /tmp/result" \
		| docker load

# Default registry for pushing images
REGISTRY ?= localhost:5050
SHA := $(shell git rev-parse --short HEAD)

push-garage: ## Push garage image to registry, clean up local copy
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

push-club: ## Push moto-club to registry, clean up local copy
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
	@echo "Cleaning up local Docker copies..."
	-docker rmi moto-club:latest $(REGISTRY)/moto-club:latest $(REGISTRY)/moto-club:$(SHA) 2>/dev/null
	@echo "Local copies removed (image lives in registry)."

push-keybox: ## Push moto-keybox to registry, clean up local copy
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
	@echo "Cleaning up local Docker copies..."
	-docker rmi moto-keybox:latest $(REGISTRY)/moto-keybox:latest $(REGISTRY)/moto-keybox:$(SHA) 2>/dev/null
	@echo "Local copies removed (image lives in registry)."

scan-garage: ## Scan garage image for vulnerabilities (requires trivy)
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

clean-images: ## Remove all moto container images
	@echo "Removing moto images..."
	-docker images --filter=reference='moto-*' -q | xargs docker rmi -f 2>/dev/null || true
	-docker images --filter=reference='*/moto-*' -q | xargs docker rmi -f 2>/dev/null || true
	@echo "Done."

clean-nix-cache: ## Remove Nix store cache volume
	@echo "Removing Nix store cache volume..."
	-docker volume rm nix-store 2>/dev/null && echo "Removed nix-store volume." || \
		echo "nix-store volume does not exist or is in use."

##@ Registry

registry-start: ## Start local Docker registry
	@echo "Starting local registry on localhost:5000..."
	@docker run -d -p 5000:5000 --name moto-registry registry:2 2>/dev/null || \
		(docker start moto-registry 2>/dev/null && echo "Registry already exists, started.") || \
		echo "Registry already running."

registry-stop: ## Stop and remove local registry
	@echo "Stopping local registry..."
	@docker stop moto-registry 2>/dev/null && docker rm moto-registry 2>/dev/null && echo "Registry stopped and removed." || \
		echo "Registry not running or already removed."

##@ Testing

test-db-up: ## Start test database (port 5433)
	@echo "Starting test database..."
	docker compose -f docker-compose.test.yml up -d --wait
	@echo "Test database ready on port 5433."

test-db-down: ## Stop test database, remove volumes
	@echo "Stopping test database..."
	docker compose -f docker-compose.test.yml down -v
	@echo "Test database stopped."

# --ignore-missing: both crates share one database, so each sees the other's migrations as "missing"
test-db-migrate: ## Run migrations against test database
	@echo "Running moto-club-db migrations..."
	cargo sqlx migrate run --source crates/moto-club-db/migrations --database-url $(TEST_DATABASE_URL) --ignore-missing
	@echo "Running moto-keybox-db migrations..."
	cargo sqlx migrate run --source crates/moto-keybox-db/migrations --database-url $(TEST_DATABASE_URL) --ignore-missing
	@echo "All migrations complete."

test-integration: test-db-down test-db-up test-db-migrate ## Fresh database cycle + integration tests
	TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test --features integration; \
	status=$$?; \
	$(MAKE) test-db-down; \
	exit $$status

test-all: ## Every test: unit + integration + ignored (K8s) — no test left behind
	@k3d cluster list 2>/dev/null | grep -q moto || { echo "Error: k3d cluster 'moto' is not running. Start it with: make dev-cluster"; exit 1; }
	$(MAKE) test-db-down test-db-up test-db-migrate
	TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test --features integration; \
	status=$$?; \
	$(MAKE) test-db-down; \
	if [ $$status -ne 0 ]; then exit $$status; fi
	cargo test -- --ignored --skip create_utun_device --skip create_tun_device --skip tun_read_write

test-ci: ## CI tests (assumes database running)
	cargo test --lib
	TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test --features integration

##@ Local Dev

# Dev database URL for moto-club
DEV_DATABASE_URL ?= postgres://moto:moto@localhost:5432/moto_club

dev: ## Start full dev stack via moto CLI
	cargo run --bin moto -- dev up

dev-cluster: ## Create k3d cluster (idempotent)
	cargo run --bin moto -- cluster init

dev-cluster-down: ## Delete the k3d cluster and local registry
	@-pkill -f 'kubectl.*port-forward.*svc/moto-club' 2>/dev/null || true
	k3d cluster delete moto
	k3d registry delete moto-registry 2>/dev/null || true

dev-up: dev-db-up dev-keybox-init dev-db-migrate ## Start postgres + keybox + club
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

dev-db-up: ## Start dev database (port 5432)
	@echo "Starting dev database..."
	docker compose up -d --wait
	@echo "Dev database ready on port 5432."

dev-db-down: ## Stop dev database
	@echo "Stopping dev database..."
	docker compose down
	@echo "Dev database stopped."

dev-db-migrate: ## Run migrations against dev database
	@echo "Running moto-club-db migrations..."
	cargo sqlx migrate run --source crates/moto-club-db/migrations --database-url $(DEV_DATABASE_URL)
	@echo "Migrations complete."

# Idempotent: skips if all keys already exist. To regenerate: rm -rf .dev/keybox && make dev-keybox-init
dev-keybox-init: ## Generate keybox keys in .dev/keybox/
	@if [ -f .dev/keybox/master.key ] && [ -f .dev/keybox/signing.key ] && [ -f .dev/keybox/service-token ]; then \
		echo "Keybox keys already exist in .dev/keybox/"; \
	else \
		mkdir -p .dev/keybox && \
		cargo run --bin moto-keybox -- init --output-dir .dev/keybox --force && \
		openssl rand -hex 32 > .dev/keybox/service-token && \
		chmod 600 .dev/keybox/service-token && \
		echo "Keybox keys generated in .dev/keybox/"; \
	fi

dev-keybox: ## Start moto-keybox-server with dev config
	MOTO_KEYBOX_BIND_ADDR=0.0.0.0:8090 \
	MOTO_KEYBOX_HEALTH_BIND_ADDR=0.0.0.0:8091 \
	MOTO_KEYBOX_MASTER_KEY_FILE=.dev/keybox/master.key \
	MOTO_KEYBOX_SVID_SIGNING_KEY_FILE=.dev/keybox/signing.key \
	MOTO_KEYBOX_DATABASE_URL=postgres://moto:moto@localhost:5432/moto_keybox \
	MOTO_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token \
	RUST_LOG=moto_keybox=debug \
	cargo run --bin moto-keybox-server

dev-garage-image: build-garage push-garage ## Build and push garage image to registry

dev-club: ## Start moto-club with dev config
	MOTO_CLUB_DATABASE_URL=postgres://moto:moto@localhost:5432/moto_club \
	MOTO_CLUB_KEYBOX_URL=http://localhost:8090 \
	MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token \
	MOTO_CLUB_KEYBOX_HEALTH_URL=http://localhost:8091 \
	MOTO_CLUB_DEV_CONTAINER_IMAGE=moto-registry:5000/moto-garage:latest \
	RUST_LOG=moto_club=debug \
	cargo run --bin moto-club

dev-down: ## Stop all dev services and database
	@echo "Stopping dev services..."
	docker compose down
	@echo "Dev services stopped."

dev-clean: ## Stop services, remove volumes and dev state
	@echo "Cleaning dev state..."
	docker compose down -v
	rm -rf .dev/
	@echo "Dev state cleaned."

##@ Deploy

deploy: deploy-images deploy-secrets deploy-system deploy-status ## Full deploy (images + secrets + system + status)

deploy-images: build-garage push-garage build-club push-club build-keybox push-keybox ## Build and push all service images

# Secret generation directory for K8s deployment
K8S_SECRETS_DIR := .dev/k8s-secrets

deploy-secrets: ## Generate and apply K8s secrets
	@if [ -f $(K8S_SECRETS_DIR)/db-password ] && [ -f $(K8S_SECRETS_DIR)/service-token ] && \
	    [ -f $(K8S_SECRETS_DIR)/master.key ] && [ -f $(K8S_SECRETS_DIR)/signing.key ]; then \
		echo "Credentials already exist in $(K8S_SECRETS_DIR)/"; \
	else \
		echo "Generating credentials in $(K8S_SECRETS_DIR)/..." && \
		mkdir -p $(K8S_SECRETS_DIR) && \
		openssl rand -hex 32 > $(K8S_SECRETS_DIR)/db-password && \
		openssl rand -hex 32 > $(K8S_SECRETS_DIR)/service-token && \
		cargo run --bin moto-keybox -- init --output-dir $(K8S_SECRETS_DIR) --force && \
		chmod 600 $(K8S_SECRETS_DIR)/* && \
		echo "Credentials generated in $(K8S_SECRETS_DIR)/"; \
	fi
	@echo "Ensuring moto-system namespace exists..."
	@kubectl create namespace moto-system --dry-run=client -o yaml | kubectl apply -f -
	@DB_PASSWORD=$$(cat $(K8S_SECRETS_DIR)/db-password) && \
	echo "Applying postgres-credentials..." && \
	kubectl -n moto-system create secret generic postgres-credentials \
		--from-literal=password="$$DB_PASSWORD" \
		--dry-run=client -o yaml | kubectl apply -f - && \
	echo "Applying keybox-keys..." && \
	kubectl -n moto-system create secret generic keybox-keys \
		--from-file=master.key=$(K8S_SECRETS_DIR)/master.key \
		--from-file=signing.key=$(K8S_SECRETS_DIR)/signing.key \
		--from-file=service-token=$(K8S_SECRETS_DIR)/service-token \
		--dry-run=client -o yaml | kubectl apply -f - && \
	echo "Applying keybox-db-credentials..." && \
	kubectl -n moto-system create secret generic keybox-db-credentials \
		--from-literal=url="postgres://moto:$$DB_PASSWORD@postgres.moto-system:5432/moto_keybox" \
		--dry-run=client -o yaml | kubectl apply -f - && \
	echo "Applying club-db-credentials..." && \
	kubectl -n moto-system create secret generic club-db-credentials \
		--from-literal=url="postgres://moto:$$DB_PASSWORD@postgres.moto-system:5432/moto_club" \
		--dry-run=client -o yaml | kubectl apply -f - && \
	echo "Applying keybox-service-token..." && \
	kubectl -n moto-system create secret generic keybox-service-token \
		--from-file=service-token=$(K8S_SECRETS_DIR)/service-token \
		--dry-run=client -o yaml | kubectl apply -f - && \
	echo "All secrets applied to moto-system namespace."

deploy-system: ## Deploy moto-system manifests (kubectl apply -k) + port-forward
	kubectl apply -k infra/k8s/moto-system/
	@# Kill any existing port-forward to moto-club
	@-pkill -f 'kubectl.*port-forward.*svc/moto-club' 2>/dev/null || true
	@echo "Starting port-forward: localhost:18080 -> svc/moto-club:8080"
	@kubectl -n moto-system port-forward svc/moto-club 18080:8080 >/dev/null 2>&1 &

deploy-status: ## Show status of moto-system pods
	@echo "Waiting for postgres rollout..."
	@kubectl -n moto-system rollout status statefulset/postgres --timeout=120s
	@echo "Waiting for moto-keybox rollout..."
	@kubectl -n moto-system rollout status deployment/moto-keybox --timeout=120s
	@echo "Waiting for moto-club rollout..."
	@kubectl -n moto-system rollout status deployment/moto-club --timeout=120s
	@echo ""
	@echo "=== Pods ==="
	@kubectl -n moto-system get pods
	@echo ""
	@echo "=== Services ==="
	@kubectl -n moto-system get services
	@echo ""
	@NOT_READY=$$(kubectl -n moto-system get pods --no-headers | grep -v "Running\|Completed" | wc -l | tr -d ' '); \
	if [ "$$NOT_READY" -gt 0 ]; then \
		echo "ERROR: $$NOT_READY pod(s) not ready."; \
		exit 1; \
	fi
	@echo "All pods healthy."

