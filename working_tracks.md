# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-club.md bug-fix

- close_session idempotent re-close returns 404 instead of 204: SessionManager converts None to NotFound, handler returns 404; fix to return Option<Session> and treat None as 204
- Fallback create_garage has no collision-retry for auto-generated names: name collision returns 409 instead of transparent retry up to 3 times with random suffix

## moto-bike.md bug-fix

- K8s manifest: add POD_NAME and POD_NAMESPACE via downward API
- K8s manifest: add RUST_LOG="info" env var
- K8s manifest: add rolling update strategy (maxSurge: 1, maxUnavailable: 0)
- K8s manifest: add container-level securityContext (readOnlyRootFilesystem, allowPrivilegeEscalation, capabilities)

## keybox.md bug-fix

(all items completed)
