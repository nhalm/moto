# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
| moto-wgtunnel.md | v0.5 | enter.rs: Device registration via moto-club API | blocked: moto-club API endpoints |
| moto-wgtunnel.md | v0.5 | enter.rs: Session creation via moto-club API | blocked: moto-club API endpoints |
| container-system.md | v0.9 | CI workflow: .github/workflows/containers.yml | future |
| container-system.md | v0.9 | Image signing: cosign keyless signing in CI | future |
| container-system.md | v0.9 | SBOM generation: trivy SBOM + cosign attestation | future |

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
