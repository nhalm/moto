# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
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
