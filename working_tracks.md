# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## makefile v0.16

(spec-only update — no code changes needed, targets already exist)

## moto-club v2.4

(spec-only update — code already matches updated spec)

## moto-club bug-fix

- GARAGE_NOT_REGISTERED swallowed as INTERNAL_ERROR: `wg.rs:600-609` maps all `session_manager.create_session()` errors to `INTERNAL_ERROR` — `GarageNotRegistered` should surface as a distinct error code
- Session creation missing ownership/expiry/termination checks: `wg.rs:527-615` extracts owner but never checks garage ownership, expiry, or termination status — spec requires `GARAGE_NOT_OWNED`, `GARAGE_EXPIRED`, `GARAGE_TERMINATED` error responses

## keybox v0.12

(spec-only update — code already matches updated spec)

## keybox bug-fix

- `/health/ready` does not check DB connection at runtime: `health.rs:69-81` only checks `is_startup_complete()` — spec requires readiness to reflect database connectivity; handler doesn't receive `State` so it structurally cannot access the DB pool


