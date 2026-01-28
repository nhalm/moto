# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
| moto-cli.md | v0.3 | moto bike build: Nix build wrapper, docker load | |
| moto-cli.md | v0.3 | moto bike build: --tag override, --push to registry | |
| moto-cli.md | v0.3 | moto bike deploy: image selection, K8s Deployment generation | |
| moto-cli.md | v0.3 | moto bike deploy: --replicas override, --wait for ready, --wait-timeout | |
| moto-cli.md | v0.3 | moto bike deploy: --namespace flag (default: current context) | |
| moto-cli.md | v0.3 | moto bike list: formatted output (NAME, STATUS, REPLICAS, AGE, IMAGE) | |
| moto-cli.md | v0.3 | moto bike list: --json output format | |
| moto-cli.md | v0.3 | moto bike logs: --follow/-f, --tail/-n, --since options | |
| moto-wgtunnel.md | v0.5 | enter.rs: Wire up moto-wgtunnel-engine to configure WireGuard tunnel | |
| moto-wgtunnel.md | v0.5 | enter.rs: Wire up MagicConn for direct UDP connection attempts | |
| moto-wgtunnel.md | v0.5 | enter.rs: Wire up DerpClient for DERP relay fallback | |
| moto-wgtunnel.md | v0.5 | enter.rs: Spawn SSH session to garage overlay IP after tunnel established | |
| moto-wgtunnel.md | v0.5 | enter.rs: Device registration via moto-club API | blocked: moto-club API endpoints |
| moto-wgtunnel.md | v0.5 | enter.rs: Session creation via moto-club API | blocked: moto-club API endpoints |
| container-system.md | v0.9 | CI workflow: .github/workflows/containers.yml | future |
| container-system.md | v0.9 | Image signing: cosign keyless signing in CI | future |
| container-system.md | v0.9 | SBOM generation: trivy SBOM + cosign attestation | future |

## Implemented

<!-- Items completed during this loop run. Bookkeeping agent will move these to tracks-history.md -->

| Spec | Version | Item |
|------|---------|------|
| moto-cli.md | v0.3 | moto bike build: bike.toml discovery (search up to git root) |
| moto-bike.md | v0.3 | infra/pkgs/moto-bike.nix: minimal base image (CA certs, tzdata, non-root user) |
| moto-bike.md | v0.3 | infra/pkgs/moto-bike.nix: mkBike helper function (base + engine binary) |
| moto-bike.md | v0.3 | Makefile: build-bike, test-bike targets |
| moto-bike.md | v0.3 | flake.nix: export moto-bike package output |

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
