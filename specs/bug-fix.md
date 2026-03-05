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

- **`close_session` idempotent re-close returns 404 instead of 204.** `postgres_stores.rs:remove_session` correctly returns `Ok(None)` for already-closed sessions, but `SessionManager::close_session` (`sessions.rs:309-313`) converts `None` to `Err(SessionError::NotFound)`, causing the HTTP handler to return 404. Spec (line 571) requires 204. Fix: have `close_session` return `Result<Option<Session>>` and let the handler treat `None` as 204 (no broadcast needed).
- **Fallback `create_garage` has no collision-retry for auto-generated names.** `garages.rs:291-356` generates a name and goes straight to DB insert — a name collision returns `GARAGE_ALREADY_EXISTS` (409). Spec requires transparent retry up to 3 times with random suffix, then `INTERNAL_ERROR`.

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
