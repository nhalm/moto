# Moto Implementation History

<!-- NEW ITEMS GO HERE -->

### 2026-01-27: dev-container.md
- infra/smoke-test.sh: container smoke tests
- infra/modules/wireguard.nix: WireGuard tools module
- infra/modules/ssh.nix: SSH server module
- infra/modules/dev-tools.nix: Rust toolchain and cargo tools module
- infra/modules/base.nix: core system tools module
- infra/pkgs/moto-garage.nix: garage container definition

### 2026-01-27: keybox.md
- Database migrations: sqlx migration setup and initial schema
- Database schema: PostgreSQL tables (secrets, secret_versions, encrypted_deks, audit_log)
- moto-keybox-cli: dev SVID issuance command
- moto-keybox: REST API endpoints (/auth/token, /secrets)
- moto-keybox: crate scaffolding (lib.rs, types, error handling)
- moto-keybox: SVID issuance (Ed25519 JWT signing, K8s token validation)
- moto-keybox: envelope encryption (KEK wraps DEK wraps secret)
- moto-keybox: ABAC policy engine (hardcoded rules for MVP)
- moto-keybox: secret storage repository (CRUD operations)
- moto-keybox: audit logging (event capture, no secret values)
- moto-keybox-client: crate scaffolding and SVID cache
- moto-keybox-client: secret fetching and auto-refresh
- moto-keybox-cli: init command (generate KEK and signing key)
- moto-keybox-cli: secret management commands (set, get, list)

### 2026-01-27: keybox.md

- moto-keybox crate: server with auth, SVID issuance, secret storage, ABAC

---

### project-structure.md v1.1 - Complete

- Cargo workspace with workspace dependencies
- rust-toolchain.toml pinning stable channel
- .cargo/config.toml with build settings and aliases
- Makefile with build/test/lint targets
- moto-common crate (Error enum, Result type, Secret<T> wrapper)
- moto-club-types crate (GarageId, GarageState, GarageInfo)
- moto-k8s crate (K8sClient, NamespaceOps trait, Labels)
- moto-garage crate (GarageMode, GarageClient)
- moto-cli crate (binary, clap parsing)

### moto-cli.md v0.3 - In Progress

- Global flags: --json/-j, --verbose/-v, --quiet/-q, --context/-c
- MOTO_JSON env var support
- Config file support (~/.config/moto/config.toml)
- Config: [output].color, [garage].ttl
- moto garage open: auto-generated names, --engine/-e, --ttl, --owner
- moto garage list: formatted output (NAME, STATUS, AGE, TTL, ENGINE)
- moto garage list: --context filter option (filter by kubectl context, "all" for all)
- moto garage close: accepts name, --force flag, confirmation prompt
- moto garage logs: --tail/-n, --since, --follow/-f streaming
- MOTOCONFIG env var (override kubeconfig)
- MOTO_NO_COLOR env var (disable colored output)
- Exit codes: differentiate 1 (general), 2 (not found), 3 (invalid input)
- moto garage enter: WireGuard tunnel to garage (uses moto-cli-wgtunnel)

### moto-wgtunnel.md v0.4 - In Progress

- moto-wgtunnel-types crate: lib.rs, keys.rs (WgPrivateKey, WgPublicKey)
- moto-wgtunnel-types crate: ip.rs (OverlayIp, GARAGE_SUBNET, CLIENT_SUBNET)
- moto-wgtunnel-types crate: peer.rs (PeerInfo, PeerAction)
- moto-wgtunnel-types crate: derp.rs (DerpMap, DerpRegion, DerpNode)
- moto-wgtunnel-derp crate: lib.rs, protocol.rs (frame encoding/decoding)
- moto-wgtunnel-derp crate: client.rs (DERP client)
- moto-wgtunnel-derp crate: map.rs (DERP server map)
- moto-wgtunnel-conn crate: lib.rs, stun.rs (STUN for NAT discovery)
- moto-wgtunnel-conn crate: endpoint.rs (endpoint selection logic)
- moto-wgtunnel-conn crate: path.rs (PathType: Direct/Derp)
- moto-wgtunnel-conn crate: magic.rs (MagicConn: UDP + DERP multiplexer)
- moto-wgtunnel-engine crate: lib.rs, config.rs (WireGuard configuration)
- moto-wgtunnel-engine crate: tunnel.rs (tunnel management with boringtun)
- moto-wgtunnel-engine crate: platform/mod.rs, platform/linux.rs, platform/macos.rs (TUN abstractions)
- moto-cli-wgtunnel crate: lib.rs, tunnel.rs (tunnel management)
- moto-cli-wgtunnel crate: status.rs (connection status command)
- moto-cli-wgtunnel crate: enter.rs (garage enter command - types only)
- enter.rs: Wire up moto-wgtunnel-engine to configure WireGuard tunnel
- moto-club-wg crate: lib.rs, ipam.rs (IP address allocation)
- moto-club-wg crate: peers.rs (peer registration)
- moto-club-wg crate: sessions.rs (tunnel session management)
- moto-club-wg crate: ssh_keys.rs (user SSH key management)
- moto-club-wg crate: derp.rs (DERP map management)
- moto-garage-wgtunnel crate: lib.rs, register.rs (register with moto-club)
- moto-garage-wgtunnel crate: health.rs (health endpoint)
- moto-garage-wgtunnel crate: daemon.rs (main daemon loop)
- moto-garage-wgtunnel crate: ssh.rs (SSH server integration)
- enter.rs: Wire up MagicConn for direct UDP connection attempts
- enter.rs: Wire up DerpClient for DERP relay fallback
- enter.rs: Spawn SSH session to garage overlay IP after tunnel established
- moto-cli-wgtunnel crate: client.rs (MotoClubClient HTTP client for API calls)
- enter.rs: Device registration via moto-club API
- enter.rs: Session creation via moto-club API

### moto-club.md v0.3 - Complete

- moto-club-types crate: GarageId, GarageState, GarageInfo
- moto-club-wg crate: lib.rs, ipam.rs, peers.rs, sessions.rs, ssh_keys.rs, derp.rs
- moto-club-db crate: lib.rs, models.rs (database layer)
- moto-club-db crate: garage_repo.rs (garage repository)
- moto-club-api crate: lib.rs, health.rs (REST API scaffolding)
- moto-club-api crate: garages.rs (garage REST endpoints)
- moto-club-api crate: wg.rs (WireGuard coordination endpoints)
- moto-club-k8s crate: lib.rs, namespace.rs, pods.rs (K8s interactions)
- moto-club-garage crate: lib.rs, service.rs, lifecycle.rs (garage service logic)
- moto-club-reconcile crate: lib.rs, garage.rs (K8s → DB reconciliation)
- moto-club binary: main.rs (compose and run server)

### dev-container.md v0.12 - Complete

- Root flake.nix with devShells.default (Rust toolchain, build deps, version control, db clients, general tools, k8s tools, connectivity)
- infra/pkgs/moto-garage.nix (container definition using dockerTools.buildLayeredImage + buildEnv)
- infra/pkgs/default.nix (exports packages)
- infra/modules/ (base.nix, ssh.nix, dev-tools.nix, wireguard.nix for container composition)
- Claude Code installation via native binary shell script (systemd service on first boot)
- infra/smoke-test.sh (smoke test script with --keep option)
- Makefile targets: build-garage, test-garage, shell-garage, push-garage
- Container image named `moto-garage`
- Docker-wrapped Nix build (works on Mac without Linux builder)
- Build verification requirement (must run build-garage && test-garage after changes)

### pre-commit.md v0.1 - Complete

- .githooks/pre-commit script with secrets detection
- .githooks/pre-commit script with Rust formatting check
- .githooks/pre-commit script with Nix syntax check
- make install target sets core.hooksPath

### makefile.md v0.5 - Complete

- install target (sets git hooks path)
- build, test, check, fmt, lint, clean, fix, ci targets
- build-garage target (Docker-wrapped Nix: runs nix build inside nixos/nix container)
- test-garage target (builds and runs smoke tests)
- shell-garage target (interactive shell in container)
- push-garage target (push to registry)
- scan-garage target (vulnerability scanning with trivy)
- clean-images target (remove moto images)
- clean-nix-cache target (remove Nix store Docker volume)
- registry-start, registry-stop targets
- .PHONY declarations for all targets

### container-system.md v0.8 - In Progress

- infra/pkgs/moto-garage.nix (garage container definition using buildLayeredImage + buildEnv)
- infra/pkgs/default.nix (exports packages)
- infra/modules/ (base.nix, ssh.nix, dev-tools.nix, wireguard.nix)
- infra/smoke-test.sh (container smoke tests)
- Root flake.nix: packages.moto-garage for Linux systems
- Makefile: build-garage, test-garage, shell-garage targets
- Makefile: clean-images target (remove moto images)
- Makefile: clean-nix-cache target (remove Nix store volume)
- Makefile: registry-start target (start local registry)
- Makefile: registry-stop target (stop local registry)
- Makefile: push-garage target (push to registry)
- Makefile: scan-garage target (scans moto-garage for vulnerabilities using trivy)
