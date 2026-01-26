# Moto Implementation Tracking

<!--
HOW TO USE THIS FILE:

1. For EACH "Ready to Rip" spec: read spec, verify section version matches
2. If spec version > section version: check spec changelog, add new items to Remaining, update header
3. If no section exists: compare spec to code, create section with Implemented and Remaining lists
4. Pick ONE item from Remaining (skip blocked items)
5. Implement it, verify with tests
6. Move item from Remaining to Implemented
7. Code is the source of truth - if unsure whether something is implemented, check the code
-->

---

## project-structure.md v1.1

**Status:** Complete

**Implemented:**
- Cargo workspace with workspace dependencies
- rust-toolchain.toml pinning stable channel
- .cargo/config.toml with build settings and aliases
- Makefile with build/test/lint targets
- moto-common crate (Error enum, Result type, Secret<T> wrapper)
- moto-club-types crate (GarageId, GarageState, GarageInfo)
- moto-k8s crate (K8sClient, NamespaceOps trait, Labels)
- moto-garage crate (GarageMode, GarageClient)
- moto-cli crate (binary, clap parsing)

---

## moto-cli.md v0.3

**Status:** In Progress

**Implemented:**
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

**Remaining:**
- bike commands (blocked: bike.md is Wrenching)
- cluster commands (blocked: no spec)

---

## moto-wgtunnel.md v0.4

**Status:** In Progress

**Implemented:**
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

**Remaining:**
- enter.rs: Get garage peer info via moto-club API (blocked: moto-club.md - needs full WireGuard peer registry implementation)

---

## moto-club.md v0.3

**Status:** Complete

**Implemented:**
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

**Remaining:**
(none - all items complete or blocked)

---

## dev-container.md v0.7

**Status:** In Progress

**Implemented:**
- Root flake.nix with devShells.default (Rust toolchain, build deps, version control, db clients, general tools, k8s tools, connectivity)
- infra/dev-container/flake.nix (container-specific flake that imports root)
- infra/dev-container/configuration.nix (NixOS system configuration with SSH, WireGuard)
- infra/dev-container/Dockerfile (builds the NixOS container image via Nix)
- Claude Code installation via native binary shell script (systemd service on first boot)
- infra/dev-container/smoke-test.sh (smoke test script with --keep option)
- Makefile targets: docker-build-garage, docker-test-garage, docker-shell-garage
- Rename container image from `moto-dev` to `moto-garage`
- Move smoke-test.sh to infra/smoke-test.sh
- Reorganize infra/: create pkgs/ and modules/ structure
- Move container definition to infra/pkgs/moto-garage.nix
- Create reusable modules in infra/modules/ (base.nix, ssh.nix, dev-tools.nix, wireguard.nix)
- Update root flake.nix: rename packages.garage to packages.moto-garage, import from infra/pkgs/
- Update Makefile targets to use new paths and image name

**Remaining:**
(none)

---

## pre-commit.md v0.1

**Status:** Complete

**Implemented:**
- .githooks/pre-commit script with secrets detection
- .githooks/pre-commit script with Rust formatting check
- .githooks/pre-commit script with Nix syntax check
- make install target sets core.hooksPath

**Remaining:**
(none)

---

## makefile.md v0.1

**Status:** Complete

**Implemented:**
- install target (sets git hooks path)
- build, test, check, fmt, lint, clean, fix, ci targets
- docker-build-moto-garage, docker-test-moto-garage, docker-shell-moto-garage targets
- .PHONY declarations for all targets

**Remaining:**
- k3s-install, k3s-start, k3s-stop, k3s-status targets (future)

---

## container-system.md v0.4

**Status:** In Progress

**Implemented:**
- infra/pkgs/moto-garage.nix (garage container definition)
- infra/pkgs/default.nix (exports packages)
- infra/smoke-test.sh (container smoke tests)
- Root flake.nix: packages.moto-garage for Linux systems
- Makefile: docker-build-moto-garage, docker-test-moto-garage, docker-shell-moto-garage
- Makefile: docker-clean target (remove moto images)
- Makefile: registry-start target (start local registry)
- Makefile: registry-stop target (stop local registry)

**Remaining:**
- Makefile: docker-push-moto-garage target
- Makefile: docker-push-local target
- Makefile: docker-scan target (requires trivy)
- infra/pkgs/moto-engine.nix (bike container - blocked: bike.md is Wrenching)
