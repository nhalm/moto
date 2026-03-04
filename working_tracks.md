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

- Namespace naming mismatch breaks token validation: `service.rs:206` and `namespace.rs:38` use `garage_id.short()` (8-char prefix) but `wg.rs:381` token validation uses full UUID — these will never match
- GARAGE_NOT_REGISTERED swallowed as INTERNAL_ERROR: `wg.rs:600-609` maps all `session_manager.create_session()` errors to `INTERNAL_ERROR` — `GarageNotRegistered` should surface as a distinct error code
- POST /api/v1/wg/devices always returns 201: `wg.rs:469` unconditionally returns `StatusCode::CREATED` — spec says re-registration of existing device should return 200
- DeviceResponse missing `created_at` field: `wg.rs:52-61` struct has only `public_key`, `overlay_ip`, `device_name` — spec requires `created_at`
- `DEVICE_NOT_OWNED` and `SESSION_NOT_OWNED` error codes not defined: `lib.rs:239-268` error_codes module is missing both — spec requires them for ownership checks (403)
- Session creation missing ownership/expiry/termination checks: `wg.rs:527-615` extracts owner but never checks garage ownership, expiry, or termination status — spec requires `GARAGE_NOT_OWNED`, `GARAGE_EXPIRED`, `GARAGE_TERMINATED` error responses
- TTL env vars not read from environment: `moto-club-garage/src/lib.rs:43-50` hardcodes TTL constants — spec says these should be configurable via `MOTO_CLUB_MIN_TTL_SECONDS`, `MOTO_CLUB_DEFAULT_TTL_SECONDS`, `MOTO_CLUB_MAX_TTL_SECONDS` env vars

## keybox v0.12

(spec-only update — code already matches updated spec)

## keybox bug-fix

- `/health/ready` does not check DB connection at runtime: `health.rs:69-81` only checks `is_startup_complete()` — spec requires readiness to reflect database connectivity; handler doesn't receive `State` so it structurally cannot access the DB pool

## testing bug-fix

- `integration` feature flag declared but gates nothing in `moto-club-api` and `moto-keybox`: both crates have `integration = []` in Cargo.toml but zero `#[cfg(feature = "integration")]` guards in source — either add gated integration tests or remove the dead feature flag

