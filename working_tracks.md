# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## makefile.md v0.19
- (spec-only) Fix `dev-down` description to "Stop postgres only"

## moto-club.md bug-fix
- Fix `close_session` spuriously incrementing `peer_version` when re-closing an already-closed session: `remove_session` calls `get_session` which finds the session even if `closed_at IS NOT NULL`, then re-executes close and `increment_peer_version` — version changes with no actual peer change

