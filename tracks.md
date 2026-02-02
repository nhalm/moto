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

## moto-club.md v1.2

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
- moto-club-db: PostgreSQL migrations for all tables (garages, wg_devices, wg_sessions, wg_garages, derp_servers)
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
- moto-club: DERP config file loading from MOTO_CLUB_DERP_CONFIG env var (moto-club-wg DerpConfigFile, load_derp_config; moto-club-db derp_server_repo with sync_from_config; main.rs startup sync)
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

**Remaining:**
(none - moto-club.md v1.2 implementation complete)

---

## moto-wgtunnel.md v0.8

**Status:** In Progress

**Implemented:**
- moto-wgtunnel-types crate: keys.rs, ip.rs, peer.rs, derp.rs
- moto-wgtunnel-derp crate: protocol.rs, client.rs, map.rs
- moto-wgtunnel-conn crate: stun.rs, endpoint.rs, path.rs, magic.rs
- moto-wgtunnel-engine crate: config.rs, tunnel.rs, platform/
- moto-cli-wgtunnel crate: tunnel.rs, status.rs, enter.rs (partial), ttyd.rs
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
(none - moto-wgtunnel.md v0.8 implementation complete)

---

## container-system.md v0.9

**Status:** In Progress

**Implemented:**
- (see tracks-history.md)

**Remaining:**
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

## makefile.md v0.5

**Status:** Complete

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

**Remaining:**
(none - makefile.md v0.5 implementation complete)

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

## garage-lifecycle.md v0.3

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

**Remaining:**
- Repo cloning: credentials from keybox (future - MVP supports public repos)

---

## keybox.md v0.2

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

**Remaining:**
- POST /admin/rotate-dek/{name} endpoint (future)
- K8s ServiceAccount JWT validation via TokenReview API (future - MVP accepts principal info directly)
- PostgreSQL-backed repository (future - currently in-memory)

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

## supporting-services.md v0.2

**Status:** In Progress

**Implemented:**
- CLI flags: `--with-postgres` and `--with-redis` on `moto garage open` command
- API: `with_postgres` and `with_redis` fields in `CreateGarageRequest` and `CreateGarageInput`
- K8s: PostgreSQL Deployment, Service, and credentials Secret (moto-club-k8s/supporting_services.rs: GaragePostgresOps trait, build_postgres_deployment, build_postgres_service, build_postgres_credentials_secret)
- K8s: Redis Deployment, Service, and credentials Secret (moto-club-k8s/supporting_services.rs: GarageRedisOps trait, build_redis_deployment, build_redis_service, build_redis_credentials_secret)

**Remaining:**
- Garage pod: Inject Postgres env vars (POSTGRES_HOST, POSTGRES_PORT, DATABASE_URL, etc.)
- Garage pod: Inject Redis env vars (REDIS_HOST, REDIS_PORT, REDIS_URL, etc.)
- Ready check: Wait for supporting service Deployments to be available before marking garage Ready
