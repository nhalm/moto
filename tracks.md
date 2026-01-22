# Moto Implementation Tracking

<!--
HOW TO USE THIS FILE:

1. Section header = "## spec-name.md vX.Y" - must match current spec version
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

## moto-cli.md v0.2

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

**Remaining:**
- garage enter (blocked: moto-wgtunnel.md crates not implemented yet)
- bike commands (blocked: bike.md is Wrenching)
- cluster commands (blocked: no spec)

---

## moto-wgtunnel.md v0.3

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

**Remaining:**
- moto-wgtunnel-engine crate: platform/mod.rs, platform/linux.rs, platform/macos.rs (TUN abstractions)
- moto-cli-wgtunnel crate: lib.rs, tunnel.rs (tunnel management)
- moto-cli-wgtunnel crate: status.rs (connection status command)
- moto-cli-wgtunnel crate: enter.rs (garage enter command)
- moto-club-wg crate: lib.rs, ipam.rs (IP address allocation)
- moto-club-wg crate: peers.rs (peer registration)
- moto-club-wg crate: sessions.rs (tunnel session management)
- moto-club-wg crate: ssh_keys.rs (user SSH key management)
- moto-club-wg crate: derp.rs (DERP map management)
- moto-garage-wgtunnel crate: lib.rs, register.rs (register with moto-club)
- moto-garage-wgtunnel crate: health.rs (health endpoint)
- moto-garage-wgtunnel crate: daemon.rs (main daemon loop)
- moto-garage-wgtunnel crate: ssh.rs (SSH server integration)
