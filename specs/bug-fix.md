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

- `moto-club-wg`: `integration` feature flag declared (`Cargo.toml` line 12) with a stub `mod integration_tests` in `ipam.rs:239-280` that contains zero actual test functions (only comments). Either add real tests or remove the dead feature flag and empty module.

## moto-club.md

- **`close_session` does not check session ownership.** `wg.rs:789` extracts owner with `let _owner = ...` but never compares it to the session's owner. `SESSION_NOT_OWNED` error code is defined but never returned. Spec requires 403 for sessions belonging to a different user.
- **`get_device` does not check device ownership.** `wg.rs:560` extracts owner with `let _owner = ...` but never compares it to the device's owner. `DEVICE_NOT_OWNED` error code is defined but never returned. Spec requires 403 for devices belonging to a different user.
- **Fallback `create_garage` writes full UUID namespace.** `garages.rs:282` uses `format!("moto-garage-{id}")` with full UUID, but `service.rs:206` and `namespace.rs:38` use `garage_id.short()` (8-char prefix). Garages created via the fallback path get mismatched namespace names.
- **Fallback TTL validation ignores `MIN_TTL_SECONDS`.** `garages.rs:253` checks `ttl_seconds <= 0` instead of `< *MIN_TTL_SECONDS`. Accepts values 1-299 that should be rejected (minimum is 300s). The `GarageService`-backed path validates correctly.
- **`MOTO_CLUB_DEV_CONTAINER_IMAGE` env var not fully wired.** `main.rs` reads the env var and passes it to `GarageK8s`, but the fallback path in `garages.rs:288` and `DEFAULT_IMAGE` constant in `lib.rs:70` still hardcode `"ghcr.io/nhalm/moto-dev:latest"`.
- **GET `/api/v1/wg/garages/{id}` returns dummy `peer_version` and `registered_at`.** `wg.rs:927-929` hardcodes `peer_version: 0` and `registered_at: Utc::now()` with comments acknowledging the gap. PostgreSQL values are never queried in this handler.

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
