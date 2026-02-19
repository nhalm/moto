# Moto Implementation Tracking

<!--
HOW TO USE THIS FILE:

1. Section header = "## spec-name.md vX.Y" - must match current spec version
2. If spec version > section version: check spec changelog, add new items to Remaining, update header
3. If no section exists: compare spec to code, create section with Implemented and Remaining lists
4. Pick ONE item from Remaining (skip blocked items)
5. Implement it, verify with tests
6. Move item from Remaining to Implemented
7. SPEC IS SOURCE OF TRUTH - if code contradicts spec, that's a Remaining item to fix (see AGENTS.md)
8. Check spec Changelog for changes that invalidate existing code
-->

---

## moto-club.md v1.8

**Status:** Complete

**Implemented:**
- moto-club-types crate: GarageId, GarageState, GarageInfo
- moto-club-wg crate: lib.rs, ipam.rs, peers.rs, sessions.rs, derp.rs (in-memory)
- moto-club-db crate: lib.rs, models.rs, garage_repo.rs (scaffolding)
- moto-club-api crate: lib.rs, health.rs, garages.rs, wg.rs (scaffolding)
- moto-club-k8s crate: lib.rs, namespace.rs, pods.rs (scaffolding)
- moto-club-garage crate: lib.rs, service.rs, lifecycle.rs (scaffolding)
- moto-club-reconcile crate: lib.rs, garage.rs (scaffolding)
- moto-club binary: main.rs (scaffolding)
- Device identity model: WireGuard public_key as primary key (spec lines 406, 1040-1046)
- moto-club-db: PostgreSQL migrations for all tables (garages, wg_devices, wg_sessions, wg_garages)
- moto-club-db: models updated for spec v1.1 (removed Attached status, WgDevice uses public_key as PK, added WgGarage model, Garage has image field)
- moto-club-db: wg_devices repository using public_key as primary key (wg_device_repo.rs)
- moto-club-db: wg_sessions repository with garage_id ON DELETE CASCADE (wg_session_repo.rs)
- moto-club-db: wg_garages repository with deterministic IP allocation (wg_garage_repo.rs)
- moto-club-api: PostgreSQL storage implementations (postgres_stores.rs: PostgresPeerStore, PostgresSessionStore)
- moto-club-api: GET /api/v1/wg/garages/{garage_id} endpoint (returns registration info for garage pods)
- moto-club-api: K8s ServiceAccount token validation for garage endpoints (moto-k8s TokenReviewOps trait, validate_garage_token helper)
- moto-club-api: GET /api/v1/wg/derp-map endpoint (returns DERP map with version for clients and garages)
- moto-club-api: Conditional GET for peers (?version= param, 304 response)
- moto-k8s: Labels use moto.dev/garage-id and moto.dev/garage-name per spec (labels.rs updated, all usages fixed)
- moto-club: Structured JSON logging (main.rs: flatten_event=true for flat JSON output per spec lines 1183-1194)
- moto-club-api: K8s namespace deletion in close flow (DELETE /api/v1/garages/{name} calls GarageK8s.delete_garage_namespace per spec lines 903-913)
- moto-club-api: GET /api/v1/info includes api_version, git_sha, features fields per spec lines 803-817
- moto-club-api: POST /api/v1/garages uses GarageService for full K8s integration
- moto-club-api: Removed unused SESSION_EXPIRED error code (spec v1.0 changelog)
- moto-club-api: GET /api/v1/wg/sessions endpoint with ?garage_id and ?all query params per spec lines 514-540
- moto-club-api: GET /health endpoint includes database, k8s, and keybox checks per spec lines 1153-1179
- moto-club-api: GET /api/v1/garages query params ?status= and ?all= per spec lines 295-300 (with INVALID_STATUS error code)
- moto-club-api: POST /api/v1/garages/{name}/extend returns ExtendTtlResponse {expires_at, ttl_remaining_seconds} per spec lines 379-386
- Remove SSH key management (v1.2 changelog: ttyd+WireGuard tunnel is sole auth boundary): moto-club-wg/src/ssh_keys.rs, moto-club-db user_ssh_key_repo.rs and user_ssh_keys table, UserSshKey model, SSH key API endpoints, PostgresSshKeyStore, moto-club-k8s secrets.rs (SshKeysSecretOps) and SSH volume mount in pods.rs, SSH key Secret step in garage service, INVALID_SSH_KEY/SSH_KEY_NOT_FOUND/SSH_KEY_NOT_OWNED error codes, ssh_key_manager in AppState
- Clean up outdated SSH comments in service.rs and garages.rs
- Create workspace PVC in garage create flow (spec v1.3 step 10: service.rs calls GarageWorkspacePvcOps.create_workspace_pvc before deploying pod)
- WireGuard keypair generation in garage create flow (spec v1.3 step 7: create wireguard-config ConfigMap and wireguard-keys Secret; GarageWireGuardOps trait, WireGuardResources struct, service.rs integration)
- Issue garage SVID from keybox in garage create flow (spec v1.3 step 8: moto-club-garage KeyboxClient, moto-club-k8s GarageSvidOps trait, service.rs integration with optional KeyboxClient)
- Fix: GET /api/v1/info features.websocket returns true (v1.4: WS /internal/wg/garages/{id}/peers implemented)
- Fix: Call create_garage_postgres() and create_garage_redis() in garage creation flow (v1.4: service.rs now calls GaragePostgresOps.create_garage_postgres and GarageRedisOps.create_garage_redis when with_postgres/with_redis are true)
- /health/ready and /health keybox integration (v1.5: checks keybox /health/ready on port 8081, returns degraded status if unreachable, adds keybox field to response; MOTO_CLUB_KEYBOX_URL env var for config; AppState.keybox_url field)
- Store garage public_key in wg_garages table during creation (v1.5: step 7 - service.rs calls wg_garage_repo::register after creating WireGuard resources, endpoints empty initially)
- Add owner field to RegisteredDevice and DeviceRegistration structs (v1.5: moto-club-wg peers.rs adds owner field, PostgresPeerStore now uses device.owner instead of hardcoded "unknown")
- Consolidate status enums (v1.6: remove GarageState and GarageInfo from moto-club-types/src/garage.rs; GarageStatus in moto-club-db/src/models.rs is now the single source of truth)
- Extract moto-club-ws crate (v1.6: WebSocket handlers moved from moto-club-api/src/wg.rs to moto-club-ws crate with PeerStreamingContext trait; AppState implements trait for peer streaming)
- Separate test files for wg.rs (v1.6: moved tests from moto-club-api/src/wg.rs to wg_test.rs per AGENTS.md test organization convention)
- Separate test files for pods.rs (v1.6: moved tests from moto-club-k8s/src/pods.rs to pods_test.rs per AGENTS.md test organization convention)
- Remove in-memory storage (v1.6: deleted InMemoryPeerStore/InMemoryStore re-exports from moto-club-api; added PostgresIpamStore; updated AppState and main.rs to use PostgreSQL storage exclusively; handler tests now require PostgreSQL)
- Simplify DERP configuration (v1.7: replace config file + database storage with MOTO_CLUB_DERP_SERVERS JSON env var; delete derp_servers table, derp_server_repo.rs, DerpServer model, DerpStore trait, DerpMapManager, InMemoryDerpStore, config file loading; add parse_derp_servers_env function; AppState uses Arc<DerpMap> instead of DerpMapManager)
- Remove InMemoryStore from moto-club-wg ipam.rs (v1.7: deleted InMemoryStore, converted tests to unit tests for pure functions only; updated lib.rs exports; added integration feature flag; updated doc examples)
- Remove InMemoryPeerStore from moto-club-wg peers.rs (v1.7: deleted InMemoryPeerStore struct and impl; removed HashMap and Mutex imports; removed export from lib.rs; existing tests are already unit tests for serialization or marked as requiring PostgreSQL)
- Remove InMemorySessionStore from moto-club-wg sessions.rs (v1.7: deleted InMemorySessionStore struct and impl; removed HashMap and Mutex imports; removed export from lib.rs; existing tests are already unit tests for Session methods and serde)
- Convert ignored integration tests to use moto-test-utils (v1.8: moto-club-api/src/wg_test.rs handler_tests module now uses `#[cfg(feature = "integration")]` instead of `#[ignore]`; tests use test_pool() for database connection and unique_owner() for test isolation)

**Remaining:**
(none - moto-club.md v1.8 implementation complete)

---

## moto-wgtunnel.md v0.9

**Status:** Complete

**Implemented:**
- moto-wgtunnel-types crate: keys.rs, ip.rs, peer.rs, derp.rs
- moto-wgtunnel-derp crate: protocol.rs, client.rs, map.rs
- moto-wgtunnel-conn crate: stun.rs, endpoint.rs, path.rs, magic.rs
- moto-wgtunnel-engine crate: config.rs, tunnel.rs, platform/
- moto-cli-wgtunnel crate: tunnel.rs, status.rs, enter.rs, ttyd.rs (complete - v0.9 updated status)
- moto-garage-wgtunnel crate: register.rs, health.rs, daemon.rs
- enter.rs: MagicConn for direct UDP
- enter.rs: DerpClient for DERP relay fallback
- enter.rs: ttyd WebSocket terminal connection (replaces SSH per spec v0.8)
- client.rs: Device registration via moto-club API (POST /api/v1/wg/devices using WG public key as device identity per spec v0.7)
- client.rs: Session creation via moto-club API (GET garage by name, POST session with garage UUID and device pubkey per spec)
- client.rs: Get garage details for session creation (GET /api/v1/garages/{name} returns garage UUID needed for session)
- tunnel.rs: Remove device_id from DeviceIdentity (per spec v0.7: WG public key IS device identity)
- Remove SSH key management from moto-garage-wgtunnel (spec v0.8: ttyd+WireGuard tunnel is sole auth boundary)
- Remove dead SSH code from moto-cli-wgtunnel/src/enter.rs (SshConfig, spawn_ssh, etc.)

**Remaining:**
(none - moto-wgtunnel.md v0.9 implementation complete)

---

## container-system.md v1.0

**Status:** In Progress

**Implemented:**
- (see tracks-history.md)

**Remaining:**
- Create `infra/pkgs/moto-keybox.nix` (bike base + moto-keybox-server binary, using mkBike helper)
- Fix `infra/pkgs/moto-club.nix` cargoHash placeholder (replace `sha256-AAAA...` with real hash)
- Export `moto-keybox-image` from flake.nix
- CI workflow: .github/workflows/containers.yml (future)
- Image signing: cosign keyless signing in CI (future)
- SBOM generation: trivy SBOM + cosign attestation (future)

---

## moto-cli.md v0.3

**Status:** Complete

**Implemented:**
- Global flags: --json/-j, --verbose/-v (counted), --quiet/-q, --context/-c, --help/-h, --version/-V
- ColorMode: auto/always/never with MOTO_NO_COLOR env var support
- Configuration: XDG config path, TOML parsing, precedence (CLI > env > config > defaults)
- moto garage open: --owner, --ttl (duration parsing, min/max validation), --engine, name auto-generation
- moto garage enter: WireGuard tunnel via moto-cli-wgtunnel, SSH session spawning
- moto garage logs: --follow/-f, --tail/-n, --since (duration parsing)
- moto garage list: --context (supports "all" for multi-context), table output with context column
- moto garage close: --force, confirmation prompt
- moto bike build: --tag (default: git sha), --push (MOTO_REGISTRY env var), Docker-wrapped Nix
- moto bike deploy: --image, --replicas, --wait, --wait-timeout, --namespace/-n
- moto bike list: --namespace/-n, table output
- moto bike logs: --follow/-f, --tail/-n, --since, --namespace
- moto cluster init: --force, k3d cluster creation, idempotent, registry setup
- moto cluster status: API health check, registry health check, JSON output
- Exit codes: 0 (success), 1 (general), 2 (not found), 3 (invalid input)
- Actionable error messages with suggestions

**Remaining:**
(none - moto-cli.md v0.3 implementation complete)

---

## dev-container.md v0.13

**Status:** Complete

**Implemented:**
- Nix dockerTools.buildLayeredImage with buildEnv wrapper
- Modular structure: infra/pkgs/moto-garage.nix, infra/modules/{base,dev-tools,terminal,wireguard}.nix
- Root flake at moto/flake.nix exports moto-garage package
- Multi-arch via eachDefaultSystem (x86_64-linux, aarch64-linux)
- Rust 1.85 stable toolchain with extensions (rust-src, rust-analyzer)
- All Rust tools: cargo-watch, cargo-nextest, cargo-audit, cargo-deny, cargo-edit, cargo-expand, mold, sccache, sqlx-cli
- System libraries: pkg-config, openssl, postgresql.lib, clang
- Version control: git, jujutsu, gh
- Database clients: postgresql, redis
- General tools: curl, jq, yq, ripgrep, fd, bat, htop, tree
- Kubernetes: kubectl, k9s, kubernetes-helm
- Node.js 22.x LTS
- Connectivity: wireguard-tools, ttyd, tmux (no openssh - WireGuard tunnel is auth boundary)
- Environment variables: WORKSPACE, CARGO_HOME, CARGO_TARGET_DIR, RUST_BACKTRACE, RUST_LOG, RUSTC_WRAPPER, RUSTFLAGS, NIX_PATH, SSL_CERT_FILE, DO_NOT_TRACK
- Container config: garage-entrypoint cmd (starts ttyd), /workspace workdir, volumes, port 7681 exposed
- Terminal daemon: ttyd on port 7681 with tmux session persistence (terminal.nix module)
- Smoke tests: infra/smoke-test.sh (core tools, terminal tools, env vars, Rust compilation)

**Remaining:**
(none - dev-container.md v0.13 implementation complete)

---

## local-cluster.md v0.1

**Status:** Complete

**Implemented:**
- moto cluster init: k3d cluster creation with moto name
- k3d create args: --api-port 6550, --port 80:80, --port 443:443, --registry-create moto-registry:5000, --disable=traefik
- Idempotent: returns success if cluster already exists (unless --force)
- Docker running check
- Wait for API ready
- moto cluster status: cluster info, API health, registry health
- JSON output format with name, type, status, api, registry
- Status values: running, stopped, not_found
- Exit codes: 0 running, 1 not running/error
- --force flag to delete and recreate

**Remaining:**
(none - local-cluster.md v0.1 implementation complete)

---

## makefile.md v0.7

**Status:** In Progress

**Implemented:**
- Setup targets: install (git hooks)
- Development targets: build, test, check, fmt, lint, clean, run, fix, ci
- Container targets: build-garage, test-garage, shell-garage, push-garage, scan-garage, clean-images, clean-nix-cache
- Bike targets: build-bike, test-bike
- Registry targets: registry-start, registry-stop
- Docker-wrapped Nix build (NIX_LINUX_SYSTEM auto-detection)
- nix-store volume for caching
- REGISTRY env var support (default: localhost:5000)
- SHA tagging from git
- .PHONY declarations for all targets
- Testing targets: test-db-up, test-db-down, test-db-migrate, test-integration, test-all

**Remaining:**
- Service container targets: build-club, push-club, build-keybox, push-keybox (blocked: container-system.md moto-keybox-image)
- Local dev targets: dev-up, dev-down, dev-clean, dev-db-up, dev-db-down, dev-db-migrate, dev-keybox-init, dev-keybox, dev-club, dev-garage-image
- Deploy targets: deploy-secrets, deploy-system, deploy-status, undeploy-system
- docker-compose.yml for dev databases (port 5432)
- scripts/init-dev-db.sql (creates moto_keybox database)
- .dev/ added to .gitignore

---

## moto-bike.md v0.3

**Status:** In Progress

**Implemented:**
- Bike base image (infra/pkgs/moto-bike.nix): CA certs, tzdata, non-root user (1000:1000), security context
- mkBike helper function for building final images from bike base + engine binary
- Flake exports moto-bike package and mkBike lib helper
- bike.toml for moto-club engine (crates/moto-club/bike.toml)
- Engine health endpoints: /health/live, /health/ready, /health/startup on port 8081 (moto-club-api health.rs, moto-club main.rs)
- Final bike images in flake: moto-club-image using mkBike helper (infra/pkgs/moto-club.nix, flake.nix exports packages.{x86_64,aarch64}-linux.moto-club-image)
- Engine Contract: Prometheus metrics endpoint on port 9090 (moto-club main.rs with metrics-exporter-prometheus, moto-club-api metrics.rs with http_requests_total and http_request_duration_seconds, process metrics via metrics-process)
- Engine Contract: Graceful shutdown (SIGTERM handling, 30s grace period) - moto-club main.rs shutdown_signal() with tokio::signal

**Remaining:**
- K8s Deployment generation from bike.toml (future - needs CLI support)

---

## garage-lifecycle.md v0.4

**Status:** In Progress

**Implemented:**
- moto garage extend CLI command: --ttl flag (default 2h), duration parsing, max TTL validation
- moto-garage: GarageClient.extend() method updates namespace labels with new expires_at
- moto-k8s: NamespaceOps.patch_namespace_labels() for updating namespace labels via merge patch
- JSON output for extend command (name, expires_at, ttl_remaining_seconds)
- Dev container: ttyd daemon on port 7681 with tmux for session persistence (garage-entrypoint script, container Cmd updated)
- moto garage enter: ttyd WebSocket client (moto-cli-wgtunnel ttyd.rs), replaces SSH with WebSocket to port 7681
- Ready criteria check: WireGuard registration check in reconciler (garage transitions to Ready only when wg_garages entry exists)
- Ready criteria check: ttyd accepting connections (K8s TCP readiness probe on port 7681 in garage pod spec)
- Repo cloning: init container with REPO_URL, REPO_BRANCH, REPO_NAME env vars (moto-club-k8s pods.rs RepoConfig, build_repo_clone_init_container); workspace volume shared between init and main container; 3-retry clone logic
- 5-state lifecycle: Rename Running to Initializing, add Failed state per spec v0.3 changelog (GarageStatus enum, GarageState enum, lifecycle state machine, reconciler mapping, API status parsing)
- CLI --branch flag for garage open (v0.4: passes branch to CreateGarageRequest)
- CLI --no-attach flag for garage open (v0.4: creates garage without connecting; default is to connect after creation)
- Ready criteria check: repo cloned (v0.4: reconciler checks init container completed successfully via init_container_succeeded method in GaragePodOps trait, moto-club-k8s pods.rs)
- Fix garage open output format to match spec (v0.4: show ID, branch, expires_at, status) - moto-cli/src/commands/garage.rs, GarageOpenJson struct updated, format_short_id and format_expires_at helpers added
- Fix garage list columns to match spec (v0.4: add ID, BRANCH columns) - moto-cli/src/commands/garage.rs, GarageJson struct updated, table header and rows now show ID, NAME, BRANCH, STATUS, TTL, AGE columns

**Remaining:**
- Repo cloning: credentials from keybox (future - MVP supports public repos)

---

## keybox.md v0.6

**Status:** In Progress

**Implemented:**
- moto-keybox library: SPIFFE ID types (garage/bike/service), SVID claims, SvidIssuer, SvidValidator
- moto-keybox: Envelope encryption (MasterKey, DataEncryptionKey, EncryptedDek, EncryptedSecret)
- moto-keybox: ABAC PolicyEngine with hardcoded policies per spec (MVP)
- moto-keybox: SecretRepository (in-memory) with CRUD operations per scope
- moto-keybox: REST API handlers (POST /auth/token, GET/POST/DELETE /secrets/{scope}/{name}, GET /secrets/{scope}, GET /audit/logs)
- moto-keybox-db: models (Secret, SecretVersion, EncryptedDek, AuditLogEntry)
- moto-keybox-db: PostgreSQL migrations (initial schema)
- moto-keybox-db: connect, run_migrations, MIGRATIONS embedded
- moto-keybox-client: KeyboxClient with K8s mode and local mode support
- moto-keybox-client: SvidCache with automatic refresh
- moto-keybox-cli: init command (generates KEK and SVID signing key)
- moto-keybox-cli: issue-dev-svid command (24h dev SVID for local testing)
- moto-keybox-cli: set/get/list secret commands
- moto-keybox-server: Server binary (main.rs) with config from env vars, graceful shutdown, JSON logging
- POST /auth/issue-garage-svid endpoint for moto-club delegation (per spec v0.3 changelog: garage SVID issuance with 1-hour TTL, service token auth, IssueGarageSvidRequest/Response types)
- Service token authentication for moto-club (MOTO_KEYBOX_SERVICE_TOKEN and MOTO_KEYBOX_SERVICE_TOKEN_FILE env vars, constant-time comparison)
- 1 MB maximum secret size limit in API validation (v0.4: MAX_SECRET_SIZE_BYTES constant, validation in set_secret handler, SECRET_TOO_LARGE error code)
- Return 403 Forbidden for both "not found" and "access denied" to prevent secret enumeration (v0.4: map_error returns ACCESS_DENIED for both SecretNotFound and AccessDenied errors, updated client to remove dead SECRET_NOT_FOUND code path)
- Health check endpoints per moto-bike.md spec (v0.4: /health/live, /health/ready, /health/startup on port 8081 via moto-keybox-server, health.rs module in moto-keybox)
- Wire up moto-keybox-db PostgreSQL backend for secrets and audit logs (v0.4: secret_repo.rs, audit_repo.rs in moto-keybox-db; PgSecretRepository in moto-keybox/pg_repository.rs; PgAppState and pg_router in moto-keybox/pg_api.rs; server uses MOTO_KEYBOX_DATABASE_URL env var to enable PostgreSQL mode)
- Fix bikes ABAC: enforce service field matching (v0.4: SvidClaims.service field added; bikes must have service claim to access service-scoped secrets; ABAC evaluate_service checks principal.service == resource.service)
- Rename moto-keybox-server binary from moto-keybox to moto-keybox-server (v0.5: fixes cargo doc collision with moto-keybox library crate)
- v0.6 integration tests: no ignored tests exist in moto-keybox or moto-keybox-db to convert (all existing tests are unit tests that don't require PostgreSQL)

**Remaining:**
- Endpoint authorization matrix enforcement (future - spec v0.3: SVID tokens should be denied for admin endpoints)
- POST /admin/rotate-dek/{name} endpoint (future - Phase 2)
- Add request logging/metrics middleware (future - Phase 2)
- K8s ServiceAccount JWT validation via TokenReview API (future - MVP accepts principal info directly)

---

## garage-isolation.md v0.3

**Status:** In Progress

**Implemented:**
- Pod security context: runAsUser/runAsGroup: 0, allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, seccompProfile: RuntimeDefault, capabilities (drop ALL, add CHOWN/DAC_OVERRIDE/FOWNER/SETGID/SETUID/NET_BIND_SERVICE)
- Pod spec: automountServiceAccountToken: false, host_network/host_pid/host_ipc: false
- Pod resource limits: 3 CPU / 7Gi per spec (requests: 100m CPU / 256Mi)
- Pod volumes: writable emptyDir mounts for tmp, var-tmp, home, nix, cargo, var-lib-apt, var-cache-apt, usr-local
- Workspace PVC: workspace volume uses PersistentVolumeClaim per spec (moto-k8s PvcOps trait, moto-club-k8s GarageWorkspacePvcOps trait, pods.rs uses PVC for /workspace mount)
- Pod volumes: wireguard-config ConfigMap mount, wireguard-keys Secret mount, garage-svid Secret mount (pods.rs volumes and volumeMounts per spec)
- NetworkPolicy: garage-isolation policy per spec (moto-k8s NetworkPolicyOps trait, moto-club-k8s GarageNetworkPolicyOps trait and build_garage_isolation_policy, integrated into GarageService.create_k8s_resources)
- ResourceQuota: garage-quota per spec (moto-k8s ResourceQuotaOps trait, moto-club-k8s GarageResourceQuotaOps trait and build_garage_quota, integrated into GarageService.create_k8s_resources)
- LimitRange: garage-limits per spec (moto-k8s LimitRangeOps trait, moto-club-k8s GarageLimitRangeOps trait and build_garage_limits, integrated into GarageService.create_k8s_resources)

**Remaining:**
(none - garage-isolation.md v0.3 implementation complete)

---

## supporting-services.md v0.3

**Status:** Complete

**Implemented:**
- CLI flags: `--with-postgres` and `--with-redis` on `moto garage open` command
- API: `with_postgres` and `with_redis` fields in `CreateGarageRequest` and `CreateGarageInput`
- K8s: PostgreSQL Deployment, Service, and credentials Secret (moto-club-k8s/supporting_services.rs: GaragePostgresOps trait, build_postgres_deployment, build_postgres_service, build_postgres_credentials_secret)
- K8s: Redis Deployment, Service, and credentials Secret (moto-club-k8s/supporting_services.rs: GarageRedisOps trait, build_redis_deployment, build_redis_service, build_redis_credentials_secret)
- Garage pod: Inject Postgres env vars (POSTGRES_HOST, POSTGRES_PORT, POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB, DATABASE_URL per spec lines 236-255)
- Garage pod: Inject Redis env vars (REDIS_HOST, REDIS_PORT, REDIS_PASSWORD, REDIS_URL per spec lines 258-272)
- Ready check: Wait for supporting service Deployments to be available before marking garage Ready (reconciler checks postgres_available/redis_available before transitioning to Ready)
- Fix: Call create_garage_postgres() and create_garage_redis() in garage creation flow (v0.3: service.rs now calls GaragePostgresOps.create_garage_postgres and GarageRedisOps.create_garage_redis when with_postgres/with_redis are true)

**Remaining:**
(none - supporting-services.md v0.3 implementation complete)

---

## project-structure.md v1.4

**Status:** Complete

**Implemented:**
- (see tracks-history.md for prior work)
- Deprecate moto-garage crate and local mode: moto-cli garage commands now use MotoClubClient HTTP client instead of moto_garage::GarageClient, removed moto-garage dependency from moto-cli, added list_garages/create_garage/close_garage/extend_garage methods to MotoClubClient
- Remove moto-garage crate entirely (v1.4: deleted crates/moto-garage/ directory, removed moto-garage from Cargo.toml workspace dependencies)

**Remaining:**
(none - project-structure.md v1.4 implementation complete)

---

## testing.md v0.4

**Status:** In Progress

**Implemented:**
- docker-compose.test.yml: PostgreSQL 16-alpine on port 5433, healthcheck, test credentials (moto_test/moto_test/moto_test)
- Makefile target: test-ci (assumes database already running, runs unit + integration tests)
- Update test target to run unit tests only (cargo test --lib)
- Add `integration` feature flag to database crates (moto-club-db, moto-club-api, moto-keybox-db, moto-keybox)
- moto-test-utils crate: test_pool(), unique_garage_name(), unique_owner(), fake_wg_pubkey()
- moto-club-db integration tests: garage_repo_test.rs (15 tests)
- moto-club-db integration tests: wg_device_repo_test.rs (13 tests)
- Makefile target: test-db-up (docker compose -f docker-compose.test.yml up -d --wait, port 5433)
- Makefile target: test-db-down (docker compose -f docker-compose.test.yml down -v)
- Makefile target: test-db-migrate (cargo sqlx migrate run with --ignore-missing for moto-club-db AND moto-keybox-db)
- Makefile target: test-integration (fresh database cycle: teardown, start, migrate, run integration tests, teardown; preserves test exit status through cleanup)
- Makefile target: test-all (unit tests via `test` target, then full `test-integration` cycle)
- Fix moto-club-api integration test compilation (19 tests: added missing imports for Arc, Body, Request, header, tower::ServiceExt, PostgresIpamStore, PostgresPeerStore, PostgresSessionStore, Ipam, PeerRegistry, SessionManager, PeerBroadcaster, DerpNode, DerpRegion to handler_tests module in wg_test.rs)

**Remaining:**
- moto-club-db integration tests: wg_session_repo_test.rs (11 public functions untested)
- moto-club-db integration tests: wg_garage_repo_test.rs (7 public functions untested)
- moto-keybox-db integration tests: secret_repo_test.rs (13 public functions untested)
- moto-keybox-db integration tests: audit_repo_test.rs (3 public functions untested)
- CI workflow: .github/workflows/test.yml (future)

---

## local-dev.md v0.1

**Status:** Not Started

**Implemented:**
(none)

**Remaining:**
- docker-compose.yml with dev Postgres on port 5432
- scripts/init-dev-db.sql (creates moto_keybox database)
- .dev/ added to .gitignore
- Makefile targets: dev-db-up, dev-db-down, dev-db-migrate
- Makefile target: dev-keybox-init (generate keys in .dev/keybox/)
- Makefile target: dev-keybox (start keybox server with dev config)
- Makefile target: dev-club (start moto-club with dev config)
- Makefile target: dev-garage-image (build + push garage to local registry)
- Makefile target: dev-up (full stack shortcut)
- Makefile target: dev-down, dev-clean (teardown)

---

## service-deploy.md v0.1

**Status:** Not Started

**Implemented:**
(none)

**Remaining:**
- infra/k8s/moto-system/namespace.yaml
- infra/k8s/moto-system/postgres.yaml (StatefulSet + Service + init ConfigMap)
- infra/k8s/moto-system/keybox.yaml (Deployment + Service)
- infra/k8s/moto-system/club.yaml (Deployment + Service + ServiceAccount + RBAC)
- infra/k8s/moto-system/kustomization.yaml
- Makefile target: deploy-secrets (generate + apply K8s secrets)
- Makefile target: deploy-system (kubectl apply -k)
- Makefile target: deploy-status, undeploy-system
- Makefile targets: build-club, push-club (blocked: container-system.md moto-club-image cargoHash)
- Makefile targets: build-keybox, push-keybox (blocked: container-system.md moto-keybox-image)

---

## pre-commit.md v0.2

**Status:** Complete

**Implemented:**
- .githooks/pre-commit: blocks secrets (.pem, .key, .env files)
- .githooks/pre-commit: cargo fmt --all --check (when Rust files changed)
- .githooks/pre-commit: cargo clippy --all-targets -- -D warnings (when Rust files changed, v0.2 changelog)
- .githooks/pre-commit: nix flake check --no-build (when Nix files changed)
- make install: sets git core.hooksPath to .githooks

**Remaining:**
(none - pre-commit.md v0.2 implementation complete)
