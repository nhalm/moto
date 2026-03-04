# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## makefile.md v0.18
- (spec-only) Fix `push-garage` comment to include "clean up local copy"
- (spec-only) Document `registry-start` vs `REGISTRY` port mismatch with override guidance
- (spec-only) Document `deploy-system` port-forward side effect

## service-deploy.md v0.6
- (spec-only) Remove stale manual port-forward from Quick path
- (spec-only) Fix binary name: `moto-keybox init` (was `moto-keybox-cli init`)

## keybox.md v0.13
- (spec-only) Fix `MOTO_KEYBOX_SERVICE_TOKEN` example value

## moto-club.md bug-fix
- Fix `state.k8s_client` always `None`: `AppState` never calls `.with_k8s_client()`, bypassing K8s SA token validation and /health/ready K8s check
- Fix `set_session` not incrementing `peer_version`: `postgres_stores.rs:321-349` creates session but never calls `wg_garage_repo::increment_peer_version`

## keybox.md bug-fix
- Fix `POST /auth/issue-garage-svid` returning 401 instead of 403 for invalid service token: add `.map_err()` wrapper like other service-token-gated endpoints
- Fix `POST /auth/token` ignoring `MOTO_KEYBOX_SVID_TTL_SECONDS`: uses hardcoded 900s instead of configured TTL

