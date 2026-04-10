.DEFAULT_GOAL := help

.PHONY: help install build test check fmt lint clean run fix audit ci
.PHONY: test-ci
.PHONY: build-garage test-garage shell-garage push-garage scan-garage clean-images
.PHONY: build-bike test-bike
.PHONY: build-club push-club build-keybox push-keybox
.PHONY: registry-start registry-stop
.PHONY: cosign-keygen sign-images sign-garage sign-club sign-keybox
.PHONY: test-db-up test-db-down test-db-migrate test-integration test-all smoke-keybox smoke-ai-proxy
.PHONY: dev dev-cluster dev-cluster-down dev-up dev-down dev-clean
.PHONY: dev-db-up dev-db-down dev-db-migrate dev-keybox-init dev-keybox dev-club dev-garage-image
.PHONY: deploy-images deploy-secrets deploy-system deploy-status deploy
.PHONY: generate-manifests

help: ## Show all available targets
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make \033[36m<target>\033[0m\n"} /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } /^[a-zA-Z_-]+:.*?## / { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

##@ Setup

install: ## Set up local development environment
	git config core.hooksPath .githooks
	cargo build --release --bin moto
	cp target/release/moto ~/.local/bin/moto

##@ Development

build: ## Build all crates
	cargo build --workspace

# Test database URLs (separate databases per service, matching production)
TEST_CLUB_DATABASE_URL ?= postgres://moto_test:moto_test@localhost:5433/moto_test_club
TEST_KEYBOX_DATABASE_URL ?= postgres://moto_test:moto_test@localhost:5433/moto_test_keybox
# Legacy variable for compatibility
TEST_DATABASE_URL ?= $(TEST_CLUB_DATABASE_URL)

test: ## Run unit tests (fast, no dependencies)
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
	cargo run --bin moto

fix: fmt ## Auto-fix lint issues
	cargo clippy --workspace --all-targets --fix --allow-dirty

audit: ## Check for known CVEs in dependencies
	@if ! command -v cargo-audit >/dev/null 2>&1 && ! test -x "$$HOME/.cargo/bin/cargo-audit"; then \
		echo "Error: cargo-audit is not installed. Install with 'cargo install cargo-audit'"; \
		exit 1; \
	fi
	PATH="$$HOME/.cargo/bin:$$PATH" cargo audit --ignore RUSTSEC-2023-0071 --ignore RUSTSEC-2025-0134

ci: test-ci ## Alias for test-ci

##@ Container

build-garage: ## Build garage container
	@echo "Building moto-garage container..."
	docker build -t moto-garage:latest -f infra/docker/Dockerfile.garage .

test-garage: build-garage ## Run smoke tests on garage container
	@echo "Running smoke tests..."
	./infra/smoke-test-garage.sh

shell-garage: ## Interactive shell in garage container
	@if ! docker image inspect moto-garage:latest &>/dev/null; then \
		echo "Image not found, building..."; \
		$(MAKE) build-garage; \
	fi
	docker run -it --rm moto-garage:latest

build-bike: ## Build moto-bike base container
	@echo "Building moto-bike container..."
	docker build -t moto-bike:latest -f infra/docker/Dockerfile.bike .

test-bike: build-bike ## Run smoke tests on bike container
	@echo "Running bike smoke tests..."
	./infra/smoke-test-bike.sh

build-club: ## Build moto-club container image
	@echo "Building moto-club container..."
	docker build -t moto-club:latest -f infra/docker/Dockerfile.club .

build-keybox: ## Build moto-keybox container image
	@echo "Building moto-keybox container..."
	docker build -t moto-keybox:latest -f infra/docker/Dockerfile.keybox .

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

##@ Registry

registry-start: ## Start local Docker registry
	@echo "Starting local registry on localhost:5050..."
	@docker run -d -p 5050:5000 --name moto-registry registry:2 2>/dev/null || \
		(docker start moto-registry 2>/dev/null && echo "Registry already exists, started.") || \
		echo "Registry already running."

registry-stop: ## Stop and remove local registry
	@echo "Stopping local registry..."
	@docker stop moto-registry 2>/dev/null && docker rm moto-registry 2>/dev/null && echo "Registry stopped and removed." || \
		echo "Registry not running or already removed."

##@ Image Signing

cosign-keygen: ## Generate cosign keypair in .dev/cosign/ (idempotent)
	@if [ -f .dev/cosign/cosign.key ] && [ -f .dev/cosign/cosign.pub ]; then \
		echo "Cosign keypair already exists in .dev/cosign/"; \
	else \
		if ! command -v cosign &>/dev/null; then \
			echo "Error: cosign is not installed. Install with 'brew install cosign' or 'nix-shell -p cosign'"; \
			exit 1; \
		fi; \
		mkdir -p .dev/cosign && \
		cd .dev/cosign && \
		COSIGN_PASSWORD="" cosign generate-key-pair && \
		chmod 600 cosign.key && \
		echo "Cosign keypair generated in .dev/cosign/"; \
	fi

sign-garage: cosign-keygen ## Sign moto-garage images in registry
	@if ! command -v cosign &>/dev/null; then \
		echo "Error: cosign is not installed. Install with 'brew install cosign' or 'nix-shell -p cosign'"; \
		exit 1; \
	fi
	@echo "Signing $(REGISTRY)/moto-garage:latest and $(REGISTRY)/moto-garage:$(SHA)..."
	cosign sign --yes --key .dev/cosign/cosign.key $(REGISTRY)/moto-garage:latest
	cosign sign --yes --key .dev/cosign/cosign.key $(REGISTRY)/moto-garage:$(SHA)
	@echo "Images signed successfully."

sign-club: cosign-keygen ## Sign moto-club images in registry
	@if ! command -v cosign &>/dev/null; then \
		echo "Error: cosign is not installed. Install with 'brew install cosign' or 'nix-shell -p cosign'"; \
		exit 1; \
	fi
	@echo "Signing $(REGISTRY)/moto-club:latest and $(REGISTRY)/moto-club:$(SHA)..."
	cosign sign --yes --key .dev/cosign/cosign.key $(REGISTRY)/moto-club:latest
	cosign sign --yes --key .dev/cosign/cosign.key $(REGISTRY)/moto-club:$(SHA)
	@echo "Images signed successfully."

sign-keybox: cosign-keygen ## Sign moto-keybox images in registry
	@if ! command -v cosign &>/dev/null; then \
		echo "Error: cosign is not installed. Install with 'brew install cosign' or 'nix-shell -p cosign'"; \
		exit 1; \
	fi
	@echo "Signing $(REGISTRY)/moto-keybox:latest and $(REGISTRY)/moto-keybox:$(SHA)..."
	cosign sign --yes --key .dev/cosign/cosign.key $(REGISTRY)/moto-keybox:latest
	cosign sign --yes --key .dev/cosign/cosign.key $(REGISTRY)/moto-keybox:$(SHA)
	@echo "Images signed successfully."

sign-images: sign-garage sign-club sign-keybox ## Sign all moto images in registry

##@ Testing

test-db-up: ## Start test database (port 5433)
	@echo "Starting test database..."
	docker compose -f docker-compose.test.yml up -d --wait
	@echo "Test database ready on port 5433."

test-db-down: ## Stop test database, remove volumes
	@echo "Stopping test database..."
	docker compose -f docker-compose.test.yml down -v
	@echo "Test database stopped."

test-db-migrate: ## Run migrations against test database
	@echo "Running moto-club-db migrations..."
	cargo sqlx migrate run --source crates/moto-club-db/migrations --database-url $(TEST_CLUB_DATABASE_URL)
	@echo "Running moto-keybox-db migrations..."
	cargo sqlx migrate run --source crates/moto-keybox-db/migrations --database-url $(TEST_KEYBOX_DATABASE_URL)
	@echo "All migrations complete."

test-integration: test-db-down test-db-up test-db-migrate ## Fresh database cycle + integration tests
	MOTO_CLUB_DATABASE_URL=$(TEST_CLUB_DATABASE_URL) \
	MOTO_KEYBOX_DATABASE_URL=$(TEST_KEYBOX_DATABASE_URL) \
	TEST_DATABASE_URL=$(TEST_CLUB_DATABASE_URL) \
	cargo test --features integration; \
	status=$$?; \
	$(MAKE) test-db-down; \
	exit $$status

test-all: ## Every test: unit + integration + ignored (K8s) — no test left behind
	@k3d cluster list 2>/dev/null | grep -q moto || { echo "Error: k3d cluster 'moto' is not running. Start it with: make dev-cluster"; exit 1; }
	$(MAKE) test-db-down test-db-up test-db-migrate
	MOTO_CLUB_DATABASE_URL=$(TEST_CLUB_DATABASE_URL) \
	MOTO_KEYBOX_DATABASE_URL=$(TEST_KEYBOX_DATABASE_URL) \
	TEST_DATABASE_URL=$(TEST_CLUB_DATABASE_URL) \
	cargo test --features integration; \
	status=$$?; \
	$(MAKE) test-db-down; \
	if [ $$status -ne 0 ]; then exit $$status; fi
	cargo test -- --ignored --skip create_utun_device --skip create_tun_device --skip tun_read_write

test-ci: fmt check lint audit ## Full CI check (fmt + check + lint + test + audit + integration)
	cargo test --lib
	MOTO_CLUB_DATABASE_URL=$(TEST_CLUB_DATABASE_URL) \
	MOTO_KEYBOX_DATABASE_URL=$(TEST_KEYBOX_DATABASE_URL) \
	TEST_DATABASE_URL=$(TEST_CLUB_DATABASE_URL) \
	cargo test --features integration

smoke-keybox: ## Smoke test keybox in k3d (port-forward, test, cleanup)
	@kubectl -n moto-system port-forward svc/moto-keybox 18090:8080 >/dev/null 2>&1 & \
	PF_PID=$$!; \
	sleep 2; \
	KEYBOX_URL=http://localhost:18090 ./infra/smoke-test-keybox.sh; \
	status=$$?; \
	kill $$PF_PID 2>/dev/null || true; \
	exit $$status

smoke-ai-proxy: ## Smoke test ai-proxy in k3d (port-forward, test, cleanup)
	@kubectl -n moto-system port-forward svc/moto-ai-proxy 18091:8080 >/dev/null 2>&1 & \
	PF_API=$$!; \
	kubectl -n moto-system port-forward svc/moto-ai-proxy 18092:8081 >/dev/null 2>&1 & \
	PF_HEALTH=$$!; \
	kubectl -n moto-system port-forward svc/moto-keybox 18090:8080 >/dev/null 2>&1 & \
	PF_KEYBOX=$$!; \
	kubectl -n moto-system port-forward svc/moto-club 18093:8080 >/dev/null 2>&1 & \
	PF_CLUB=$$!; \
	sleep 2; \
	AI_PROXY_URL=http://localhost:18091 \
	AI_PROXY_HEALTH_URL=http://localhost:18092 \
	KEYBOX_URL=http://localhost:18090 \
	CLUB_URL=http://localhost:18093 \
	./infra/smoke-test-ai-proxy.sh; \
	status=$$?; \
	kill $$PF_API $$PF_HEALTH $$PF_KEYBOX $$PF_CLUB 2>/dev/null || true; \
	exit $$status

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
	MOTO_CLUB_BIND_ADDR=0.0.0.0:18080 \
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
		chmod 600 .dev/keybox/master.key .dev/keybox/signing.key .dev/keybox/service-token && \
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
	MOTO_CLUB_BIND_ADDR=0.0.0.0:18080 \
	MOTO_CLUB_DATABASE_URL=postgres://moto:moto@localhost:5432/moto_club \
	MOTO_CLUB_KEYBOX_URL=http://localhost:8090 \
	MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token \
	MOTO_CLUB_KEYBOX_HEALTH_URL=http://localhost:8091 \
	MOTO_CLUB_DEV_CONTAINER_IMAGE=moto-registry:5000/moto-garage:latest \
	RUST_LOG=moto_club=debug \
	cargo run --bin moto-club

dev-down: ## Stop postgres only
	@echo "Stopping dev services..."
	docker compose down
	@echo "Dev services stopped."

dev-clean: ## Stop services, remove volumes and dev state
	@echo "Cleaning dev state..."
	docker compose down -v
	rm -rf .dev/
	@echo "Dev state cleaned."

##@ Deploy

generate-manifests: ## Regenerate K8s manifests from bike.toml
	./scripts/generate-manifests.sh

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

