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

- **`peer_broadcaster` never called on session create/close.** The `PeerBroadcaster` is wired into `AppState` and the WebSocket handler at `wg.rs:1166` uses it for subscriptions, but `create_session` and `close_session` never call `broadcast_add()` or `broadcast_remove()`. Garages connected via `WS /internal/wg/garages/{id}/peers` receive no events when sessions change.
- **`close_session` spuriously increments `peer_version` when re-closing an already-closed session.** `postgres_stores.rs:remove_session` calls `get_session` which finds the session even if `closed_at IS NOT NULL`, then re-executes `wg_session_repo::close()` (idempotent via `COALESCE`) and `increment_peer_version`. Spec says "Idempotent: closing already-closed session returns 204" — the 204 is correct but the version bump is a semantic bug (version changes with no actual peer change).

## keybox.md

(none)

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
