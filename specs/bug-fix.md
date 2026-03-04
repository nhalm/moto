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

- `integration` feature flag declared but gates nothing in `moto-club-api` and `moto-keybox`. Both crates have `integration = []` in Cargo.toml but zero `#[cfg(feature = "integration")]` guards in source. Either add gated integration tests or remove the dead feature flag.

## moto-club.md

- **Namespace naming mismatch breaks token validation.** `service.rs:206` and `namespace.rs:38` use `garage_id.short()` (8-char prefix) for namespace names (e.g., `moto-garage-01963e4a`), but `wg.rs:381` token validation uses full UUID (e.g., `moto-garage-01963e4a-...`). These will never match. Either use full UUID everywhere or use short ID everywhere.
- **GARAGE_NOT_REGISTERED swallowed as INTERNAL_ERROR.** `wg.rs:600-609` maps all `session_manager.create_session()` errors to `INTERNAL_ERROR`. The `GarageNotRegistered` variant from `moto-club-wg/sessions.rs` should surface as a distinct error code.
- **POST /api/v1/wg/devices always returns 201.** `wg.rs:469` unconditionally returns `StatusCode::CREATED`. Spec says re-registration of existing device should return 200.
- **DeviceResponse missing `created_at` field.** `wg.rs:52-61` struct has only `public_key`, `overlay_ip`, `device_name`. Spec requires `created_at`.
- **`DEVICE_NOT_OWNED` and `SESSION_NOT_OWNED` error codes not defined.** `lib.rs:239-268` error_codes module is missing both. Spec requires them for ownership checks (403).
- **Session creation missing ownership/expiry/termination checks.** `wg.rs:527-615` `create_session` extracts owner (`let _owner = ...`) but never checks garage ownership, expiry, or termination status. Spec requires `GARAGE_NOT_OWNED`, `GARAGE_EXPIRED`, `GARAGE_TERMINATED` error responses.
- **TTL env vars not read from environment.** `moto-club-garage/src/lib.rs:43-50` hardcodes `DEFAULT_TTL_SECONDS`, `MAX_TTL_SECONDS`, `MIN_TTL_SECONDS` as constants. Spec says these should be configurable via `MOTO_CLUB_MIN_TTL_SECONDS`, `MOTO_CLUB_DEFAULT_TTL_SECONDS`, `MOTO_CLUB_MAX_TTL_SECONDS` env vars.

## keybox.md

- **`/health/ready` does not check DB connection at runtime.** `health.rs:69-81` `ready_handler` only checks `is_startup_complete()` (a static `AtomicBool`). It does not ping the database pool. Spec requires readiness to reflect database connectivity. The handler doesn't even receive `State`, so it structurally cannot access the DB pool — needs to be wired up.

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

- **chmod 600 only applied to `service-token`, not `master.key`/`signing.key`.** Both the Makefile `dev-keybox-init` target and `moto dev up` (`dev.rs` `ensure_keybox_keys()`) only `chmod 600` the service-token file. The `master.key` and `signing.key` files get whatever permissions `moto-keybox init` assigns. Spec says all three key files should be 600.

## service-deploy.md

(none)
