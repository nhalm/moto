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

**Remaining:**
- moto-club-k8s: Labels must use moto.dev/garage-id not moto.dev/id (check spec line 871)
- moto-club-garage: Wire up K8s operations in create flow (12 steps per spec)
- moto-club-garage: Integrate K8s namespace deletion in close flow
- moto-club: DERP config file loading (MOTO_CLUB_DERP_CONFIG env var)
- moto-club: Structured JSON logging

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

**Remaining:**
- enter.rs: Device registration via moto-club API (blocked: moto-club API PostgreSQL storage)
- enter.rs: Session creation via moto-club API (blocked: moto-club API)
- enter.rs: Get garage peer info via moto-club API (blocked: moto-club API)

---

## container-system.md v0.9

**Status:** In Progress

**Implemented:**
- (see tracks-history.md)

**Remaining:**
- CI workflow: .github/workflows/containers.yml (future)
- Image signing: cosign keyless signing in CI (future)
- SBOM generation: trivy SBOM + cosign attestation (future)
