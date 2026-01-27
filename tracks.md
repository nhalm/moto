# Moto Implementation Tracking

## Remaining

| Spec | Version | Item | Status |
|------|---------|------|--------|
| moto-cli.md | v0.3 | bike commands | blocked: bike.md is Wrenching |
| moto-cli.md | v0.3 | cluster commands | blocked: no spec |
| moto-wgtunnel.md | v0.4 | enter.rs: Get garage peer info via moto-club API | blocked: moto-club.md |
| makefile.md | v0.5 | k3s-install, k3s-start, k3s-stop, k3s-status targets | future |
| container-system.md | v0.8 | infra/pkgs/moto-engine.nix (bike container) | blocked: bike.md is Wrenching |

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
