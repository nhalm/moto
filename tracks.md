# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
| moto-cli.md | v0.3 | bike commands | blocked: bike.md is Wrenching |
| moto-cli.md | v0.3 | cluster commands | blocked: no spec |
| moto-wgtunnel.md | v0.4 | enter.rs: Get garage peer info via moto-club API | blocked: moto-club.md |
| makefile.md | v0.5 | k3s-install, k3s-start, k3s-stop, k3s-status targets | future |
| container-system.md | v0.8 | infra/pkgs/moto-engine.nix (bike container) | blocked: bike.md is Wrenching |
| keybox.md | v0.2 | moto-keybox: ABAC policy engine (hardcoded rules for MVP) | |
| keybox.md | v0.2 | moto-keybox: secret storage repository (CRUD operations) | |
| keybox.md | v0.2 | moto-keybox: REST API endpoints (/auth/token, /secrets) | |
| keybox.md | v0.2 | moto-keybox: audit logging (event capture, no secret values) | |
| keybox.md | v0.2 | moto-keybox-client: crate scaffolding and SVID cache | |
| keybox.md | v0.2 | moto-keybox-client: secret fetching and auto-refresh | |
| keybox.md | v0.2 | moto-keybox-cli: init command (generate KEK and signing key) | |
| keybox.md | v0.2 | moto-keybox-cli: secret management commands (set, get, list) | |
| keybox.md | v0.2 | moto-keybox-cli: dev SVID issuance command | |
| keybox.md | v0.2 | Database schema: PostgreSQL tables (secrets, secret_versions, encrypted_deks, audit_log) | |
| keybox.md | v0.2 | Database migrations: sqlx migration setup and initial schema | |
| dev-container.md | v0.12 | infra/pkgs/moto-garage.nix: garage container definition | |
| dev-container.md | v0.12 | infra/modules/base.nix: core system tools module | |
| dev-container.md | v0.12 | infra/modules/dev-tools.nix: Rust toolchain and cargo tools module | |
| dev-container.md | v0.12 | infra/modules/ssh.nix: SSH server module | |
| dev-container.md | v0.12 | infra/modules/wireguard.nix: WireGuard tools module | |
| dev-container.md | v0.12 | infra/smoke-test.sh: container smoke tests | |

## Implemented

<!-- Items completed during this loop run. Bookkeeping agent will move these to tracks-history.md -->

| Spec | Version | Item |
|------|---------|------|
| keybox.md | v0.2 | moto-keybox: crate scaffolding (lib.rs, types, error handling) |
| keybox.md | v0.2 | moto-keybox: SVID issuance (Ed25519 JWT signing, K8s token validation) |
| keybox.md | v0.2 | moto-keybox: envelope encryption (KEK wraps DEK wraps secret) |

---

## Workflow

1. Pick first non-blocked, non-future item from Remaining
2. Read the spec file, implement it, verify with tests
3. Move the row from Remaining to Implemented
4. Commit changes

**If only blocked/future items remain:** Output `LOOP_COMPLETE: true`

## Notes

- **Blocked:** Skip. Dependency must reach "Ready to Rip" first.
- **Future:** Skip. Belongs to a later phase.
- **Version mismatch:** If spec version > table version, check changelog for new items.
