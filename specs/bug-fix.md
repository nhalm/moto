# Bug Fix Punch List

Living punch list for cross-cutting code bugs, wiring omissions, and small
fixes that don't warrant a spec version bump. Items are grouped by owning spec.

Loop agents: when implementing a spec, check this file for items under that
spec's heading. Fix one per iteration. Delete the item from this file after
fixing and committing.

Items marked `(blocked: ...)` can't be fixed until their dependency resolves —
same convention as tracks.md.

---

## project-structure.md

(none)

## moto-cli.md

(none)

## jj-workflow.md

(none)

## pre-commit.md

(none)

## makefile.md

(none)

## testing.md

(none)

## moto-club.md

- **`delete_garage` does not close active sessions or broadcast peer removals.** `garages.rs:617-644` terminates the garage (DB status + K8s namespace delete) but never calls `session_manager.on_garage_terminated()` or `peer_broadcaster.broadcast_remove()`. Active WireGuard sessions remain open in `wg_sessions`, garages are not notified via WebSocket, and `peer_version` is not incremented at delete time.
- **Session TTL not capped to garage's remaining TTL.** `sessions.rs:259-262` uses the requested TTL (or default) without comparing to the garage's `expires_at`. Spec requires: "If requested TTL exceeds garage remaining TTL, it's capped (session can't outlive garage)." The garage's expiry is not passed to `SessionManager::create_session`.
- **`register_garage` returns FK error instead of `GARAGE_NOT_FOUND` 404.** `wg.rs:871-916` calls `peer_registry.register_garage()` without first verifying the garage exists in the `garages` table. If the UUID doesn't match, the `ON CONFLICT` upsert hits a FK constraint violation surfaced as a generic storage error, not the spec-mandated 404.

## keybox.md

- **`POST /auth/token` cannot set `service` claim for bikes.** `TokenRequest` (`api.rs:188`) has `principal_type`, `principal_id`, and `pod_uid` but no `service` field. Bikes calling `POST /auth/token` cannot obtain an SVID with a `service` claim, so they can never pass ABAC for service-scoped secrets. The `SvidClaims::with_service()` method exists but is unreachable through the token endpoint.

## dev-container.md

(none)

## container-system.md

(none)

## local-cluster.md

(none)

## garage-isolation.md

(none)

## garage-lifecycle.md

(none)

## moto-bike.md

(none)

## supporting-services.md

(none)

## moto-wgtunnel.md

(none)

## local-dev.md

(none)

## service-deploy.md

(none)
