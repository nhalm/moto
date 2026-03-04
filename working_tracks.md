# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-club bug-fix

- Session TTL not capped to garage's remaining TTL — `sessions.rs` must compare requested TTL to garage's `expires_at` and cap accordingly
- `register_garage` returns FK error instead of `GARAGE_NOT_FOUND` 404 — must verify garage exists in `garages` table before upserting into `wg_garages`

## keybox bug-fix

- `POST /auth/token` cannot set `service` claim for bikes — `TokenRequest` needs a `service` field so bikes can obtain SVIDs with service claims for ABAC
