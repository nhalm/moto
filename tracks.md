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

## moto-club.md v1.1

**Status:** In Progress

**Implemented:**
- moto-club-types crate: GarageId, GarageState, GarageInfo
- moto-club-wg crate: lib.rs, ipam.rs, peers.rs, sessions.rs, ssh_keys.rs, derp.rs (in-memory)
- moto-club-db crate: lib.rs, models.rs, garage_repo.rs (scaffolding)
- moto-club-api crate: lib.rs, health.rs, garages.rs, wg.rs (scaffolding)
- moto-club-k8s crate: lib.rs, namespace.rs, pods.rs (scaffolding)
- moto-club-garage crate: lib.rs, service.rs, lifecycle.rs (scaffolding)
- moto-club-reconcile crate: lib.rs, garage.rs (scaffolding)
- moto-club binary: main.rs (scaffolding)
- Device identity model: WireGuard public_key as primary key (spec lines 406, 1040-1046)
- moto-club-db: PostgreSQL migrations for all tables (garages, wg_devices, wg_sessions, wg_garages, user_ssh_keys, derp_servers)
- moto-club-db: models updated for spec v1.1 (removed Attached status, WgDevice uses public_key as PK, added WgGarage model, Garage has image field)
- moto-club-db: wg_devices repository using public_key as primary key (wg_device_repo.rs)
- moto-club-db: wg_sessions repository with garage_id ON DELETE CASCADE (wg_session_repo.rs)
- moto-club-db: wg_garages repository with deterministic IP allocation (wg_garage_repo.rs)
- moto-club-db: user_ssh_keys repository (user_ssh_key_repo.rs)
- moto-club-api: PostgreSQL storage implementations (postgres_stores.rs: PostgresPeerStore, PostgresSessionStore, PostgresSshKeyStore)
- moto-club-api: GET /api/v1/wg/garages/{garage_id} endpoint (returns registration info for garage pods)
- moto-club-api: K8s ServiceAccount token validation for garage endpoints (moto-k8s TokenReviewOps trait, validate_garage_token helper)
- moto-club-api: GET /api/v1/wg/derp-map endpoint (returns DERP map with version for clients and garages)
- moto-club-api: Conditional GET for peers (?version= param, 304 response)
- moto-club-k8s: SSH keys Secret creation (secrets.rs: SshKeysSecretOps trait, creates ssh-keys Secret with authorized_keys)
- moto-club-k8s: Pod SSH keys volume mount (pods.rs: mounts ssh-keys Secret to /home/moto/.ssh with mode 0600)
- moto-k8s: Labels use moto.dev/garage-id and moto.dev/garage-name per spec (labels.rs updated, all usages fixed)
- moto-club: DERP config file loading from MOTO_CLUB_DERP_CONFIG env var (moto-club-wg DerpConfigFile, load_derp_config; moto-club-db derp_server_repo with sync_from_config; main.rs startup sync)
- moto-club: Structured JSON logging (main.rs: flatten_event=true for flat JSON output per spec lines 1183-1194)
- moto-club-garage: SSH keys Secret creation wired into create flow (step 8 per spec lines 866-879; queries user_ssh_key_repo, creates Secret before pod deployment)
- moto-club-api: K8s namespace deletion in close flow (DELETE /api/v1/garages/{name} calls GarageK8s.delete_garage_namespace per spec lines 903-913)

**Remaining:**
(none - moto-club.md v1.1 implementation complete)

---

## moto-wgtunnel.md v0.7

**Status:** In Progress

**Implemented:**
- moto-wgtunnel-types crate: keys.rs, ip.rs, peer.rs, derp.rs
- moto-wgtunnel-derp crate: protocol.rs, client.rs, map.rs
- moto-wgtunnel-conn crate: stun.rs, endpoint.rs, path.rs, magic.rs
- moto-wgtunnel-engine crate: config.rs, tunnel.rs, platform/
- moto-cli-wgtunnel crate: tunnel.rs, status.rs, enter.rs (partial)
- moto-garage-wgtunnel crate: register.rs, health.rs, daemon.rs, ssh.rs
- enter.rs: MagicConn for direct UDP
- enter.rs: DerpClient for DERP relay fallback
- enter.rs: SSH session spawning
- client.rs: Device registration via moto-club API (POST /api/v1/wg/devices using WG public key as device identity per spec v0.7)
- client.rs: Session creation via moto-club API (GET garage by name, POST session with garage UUID and device pubkey per spec)
- client.rs: Get garage details for session creation (GET /api/v1/garages/{name} returns garage UUID needed for session)

**Remaining:**
(none - moto-wgtunnel.md v0.7 CLI integration complete)

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

## dev-container.md v0.12

**Status:** Complete

**Implemented:**
- Nix dockerTools.buildLayeredImage with buildEnv wrapper
- Modular structure: infra/pkgs/moto-garage.nix, infra/modules/{base,dev-tools,ssh,wireguard}.nix
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
- Connectivity: wireguard-tools, openssh
- Environment variables: WORKSPACE, CARGO_HOME, CARGO_TARGET_DIR, RUST_BACKTRACE, RUST_LOG, RUSTC_WRAPPER, RUSTFLAGS, NIX_PATH, SSL_CERT_FILE, DO_NOT_TRACK
- Container config: /bin/bash cmd, /workspace workdir, volumes, port 22 exposed
- Smoke tests: infra/smoke-test.sh (core tools, env vars, Rust compilation)

**Remaining:**
(none - dev-container.md v0.12 implementation complete)

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
