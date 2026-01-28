# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
| moto-wgtunnel.md | v0.4 | moto-club-api: /api/v1/wg/garages endpoint (garage WG registration) | |
| moto-wgtunnel.md | v0.4 | moto-club-api: /internal/wg/garages/{id}/peers WebSocket (peer streaming) | |
| moto-wgtunnel.md | v0.4 | moto-club-api: /api/v1/users/ssh-keys endpoint (SSH key registration) | |
| moto-cli.md | v0.3 | moto bike build: bike.toml discovery, container image build | blocked: bike.md Wrenching |
| moto-cli.md | v0.3 | moto bike build: --tag override, --push flag | blocked: bike.md Wrenching |
| moto-cli.md | v0.3 | moto bike deploy: image selection, replica override, --wait | blocked: bike.md Wrenching |
| moto-cli.md | v0.3 | moto bike list: formatted output, JSON output | blocked: bike.md Wrenching |
| moto-cli.md | v0.3 | moto bike logs: --follow, --tail, --since options | blocked: bike.md Wrenching |
| container-system.md | v0.8 | infra/pkgs/moto-engine.nix: minimal runtime container | blocked: bike.md Wrenching |
| container-system.md | v0.8 | CI workflow: .github/workflows/containers.yml | future |
| container-system.md | v0.8 | Image signing: cosign keyless signing in CI | future |
| container-system.md | v0.8 | SBOM generation: trivy SBOM + cosign attestation | future |

## Implemented

<!-- Items completed during this loop run. Bookkeeping agent will move these to tracks-history.md -->

| Spec | Version | Item |
|------|---------|------|
| moto-wgtunnel.md | v0.4 | moto-club-api: /api/v1/wg/sessions endpoint (session creation) |
| moto-wgtunnel.md | v0.4 | moto-club-api: /api/v1/wg/devices endpoint (device registration) |
| local-cluster.md | v0.1 | cluster status: JSON output format |
| local-cluster.md | v0.1 | cluster status: registry health check |
| local-cluster.md | v0.1 | cluster status: K8s API health check |
| local-cluster.md | v0.1 | cluster status: check if cluster exists via k3d |
| local-cluster.md | v0.1 | crates/moto-cli/src/commands/cluster.rs: module scaffolding |
| local-cluster.md | v0.1 | cluster init: Docker running check |
| local-cluster.md | v0.1 | cluster init: k3d cluster create command execution |
| local-cluster.md | v0.1 | cluster init: idempotent handling (cluster already exists) |
| local-cluster.md | v0.1 | cluster init: wait for K8s API ready |
| local-cluster.md | v0.1 | cluster init: --force flag (delete and recreate) |

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
