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

- **`close_session` idempotent re-close triggers spurious `broadcast_remove`.** `wg.rs:840-858` calls `peer_broadcaster.broadcast_remove()` even when the session was already closed. `postgres_stores.rs:remove_session` correctly skips the DB close and `peer_version` increment for already-closed sessions, but returns `Ok(Some(session))` without indicating it was a no-op. Fix: check `closed_at` on the returned session before broadcasting, or have `remove_session` signal whether it actually closed.
- **`extend_ttl` max-TTL guard uses original `ttl_seconds` not actual total.** `garages.rs:750-763` checks `garage.ttl_seconds + req.seconds` against `MAX_TTL_SECONDS`, but `garage.ttl_seconds` is the original creation TTL, not `(expires_at - created_at)`. If the garage was previously extended, the guard uses a stale value — can allow or reject incorrectly. Fix: compute `(garage.expires_at + Duration::seconds(req.seconds) - garage.created_at).num_seconds()`.
- **Fallback name validation missing start/end alphanumeric + 63-char limit.** `garages.rs:271-282` checks character set (lowercase + digits + hyphens) but doesn't enforce that name must start/end with alphanumeric or respect K8s 63-char label limit. Names like `-foo-` pass validation.

## keybox.md

- **`with_repository()` constructor hardcodes `admin_service` to `"moto-club"`.** `api.rs:97-110` secondary constructor ignores `MOTO_KEYBOX_ADMIN_SERVICE` env var. Tests and integration paths using `with_repository()` always use `"moto-club"` regardless of configuration. The primary `AppState::new()` constructor handles it correctly.

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
