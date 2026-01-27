# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
| keybox.md | v0.2 | moto-keybox: crate scaffolding, types, error handling | |
| keybox.md | v0.2 | moto-keybox: SPIFFE ID types and SVID JWT structure | |
| keybox.md | v0.2 | moto-keybox: K8s TokenReview authentication | |
| keybox.md | v0.2 | moto-keybox: SVID issuance with Ed25519 signing | |
| keybox.md | v0.2 | moto-keybox: envelope encryption (KEK wraps DEK wraps secret) | |
| keybox.md | v0.2 | moto-keybox: secret storage repository (CRUD operations) | |
| keybox.md | v0.2 | moto-keybox: ABAC policy engine (hardcoded policies) | |
| keybox.md | v0.2 | moto-keybox: PostgreSQL schema and migrations | |
| keybox.md | v0.2 | moto-keybox: REST API endpoints (/auth, /secrets) | |
| keybox.md | v0.2 | moto-keybox: audit logging (no values logged) | |
| keybox.md | v0.2 | moto-keybox-client: crate scaffolding and types | |
| keybox.md | v0.2 | moto-keybox-client: SVID cache and auto-refresh | |
| keybox.md | v0.2 | moto-keybox-client: secret fetch API with SecretString | |
| keybox.md | v0.2 | moto-keybox-client: K8s and local dev mode support | |
| keybox.md | v0.2 | moto-keybox-cli: crate scaffolding and CLI structure | |
| keybox.md | v0.2 | moto-keybox-cli: init command (generate KEK and signing key) | |
| keybox.md | v0.2 | moto-keybox-cli: secret management commands (set, get, list) | |
| keybox.md | v0.2 | moto-keybox-cli: issue-dev-svid command for local development | |
| dev-container.md | v0.12 | infra/modules/base.nix: core system tools | |
| dev-container.md | v0.12 | infra/modules/dev-tools.nix: Rust toolchain and cargo tools | |
| dev-container.md | v0.12 | infra/modules/ssh.nix: OpenSSH server configuration | |
| dev-container.md | v0.12 | infra/modules/wireguard.nix: WireGuard tools | |
| dev-container.md | v0.12 | infra/pkgs/moto-garage.nix: container image definition | |
| dev-container.md | v0.12 | infra/smoke-test.sh: smoke test script for garage container | |
| local-cluster.md | v0.1 | moto-cli: cluster init command (k3d cluster create) | |
| local-cluster.md | v0.1 | moto-cli: cluster status command (health checks) | |
| local-cluster.md | v0.1 | moto-cli: Docker runtime detection and error handling | |
| moto-cli.md | v0.3 | moto-cli: crate scaffolding, clap CLI structure | |
| moto-cli.md | v0.3 | moto-cli: global flags (--json, --verbose, --quiet, --context) | |
| moto-cli.md | v0.3 | moto-cli: configuration file support (XDG_CONFIG_HOME) | |
| moto-cli.md | v0.3 | moto-cli: garage open command | |
| moto-cli.md | v0.3 | moto-cli: garage enter command | |
| moto-cli.md | v0.3 | moto-cli: garage logs command | |
| moto-cli.md | v0.3 | moto-cli: garage list command | |
| moto-cli.md | v0.3 | moto-cli: garage close command | |
| moto-cli.md | v0.3 | moto-cli: bike commands | blocked: bike.md is Wrenching |
| moto-cli.md | v0.3 | moto-cli: cluster commands integration | |
| moto-cli.md | v0.3 | moto-cli: error handling with actionable suggestions | |
| moto-wgtunnel.md | v0.4 | moto-cli: WireGuard engine integration in enter command | |
| moto-wgtunnel.md | v0.4 | moto-cli: direct UDP connection with MagicConn | |
| moto-wgtunnel.md | v0.4 | moto-cli: DERP relay fallback with DerpClient | |
| moto-wgtunnel.md | v0.4 | moto-cli: SSH session spawning over tunnel | |
| moto-wgtunnel.md | v0.4 | moto-cli: device registration endpoint integration | blocked: moto-club.md |
| moto-wgtunnel.md | v0.4 | moto-cli: session creation endpoint integration | blocked: moto-club.md |
| moto-wgtunnel.md | v0.4 | moto-garage-wgtunnel: garage peer info endpoint integration | blocked: moto-club.md |
| makefile.md | v0.5 | Makefile: install target (git hooks setup) | |
| makefile.md | v0.5 | Makefile: development targets (build, test, check, fmt, lint, clean, fix, ci) | |
| makefile.md | v0.5 | Makefile: container targets (build-garage, test-garage, shell-garage, push-garage) | |
| makefile.md | v0.5 | Makefile: registry targets (registry-start, registry-stop) | |
| makefile.md | v0.5 | Makefile: scan-garage and clean-images targets | |
| makefile.md | v0.5 | Makefile: clean-nix-cache target | |
| makefile.md | v0.5 | Makefile: k3s targets (k3s-install, k3s-start, k3s-stop, k3s-status) | future |
| container-system.md | v0.8 | infra/pkgs/moto-engine.nix: minimal runtime container | blocked: bike.md is Wrenching |

## Implemented

<!-- Items completed during this loop run. Bookkeeping agent will move these to tracks-history.md -->

| Spec | Version | Item |
|------|---------|------|

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
