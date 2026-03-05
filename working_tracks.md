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

(all items completed)

## container-system.md bug-fix

- Makefile: fix registry-start port from 5000 to 5050

## moto-bike.md bug-fix

- bike.toml: update replicas from 2 to 3
- K8s manifest: add POD_NAME and POD_NAMESPACE via downward API
- K8s manifest: add RUST_LOG="info" env var
- K8s manifest: add rolling update strategy (maxSurge: 1, maxUnavailable: 0)
- K8s manifest: add container-level securityContext (readOnlyRootFilesystem, allowPrivilegeEscalation, capabilities)

## keybox.md bug-fix

(all items completed)
