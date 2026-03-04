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

- Fix `test` target description: "Run unit tests" (was misleadingly "Run all tests")
- Add `build-bike` and `test-bike` to Container Targets spec section (spec-only — already in Makefile per v0.6)
- Remove stale `localhost:5000` from `push-garage` comment (registry port is determined by REGISTRY variable)

## makefile v0.17

(spec-only update — no code changes needed, targets already exist)

## keybox v0.12

(spec-only update — documents K8s TokenReview as MVP-deferred, garage ABAC policies, DATABASE_URL as optional, extra list endpoints, SERVICE_TOKEN env var, ADMIN_SERVICE env var)

## moto-club v2.4

(spec-only update — fixes pod_name in examples, /health response format, /health/ready behavior, documents version field in derp-map response)

## moto-club v2.5

(spec-only update — namespace format documented as short_id)

## moto-club bug-fix

- Fallback `create_garage` writes full UUID namespace. `garages.rs` uses `format!("moto-garage-{id}")` with full UUID, but `service.rs` and `namespace.rs` use `garage_id.short()` (8-char prefix). Garages created via the fallback path get mismatched namespace names.
- `MOTO_CLUB_DEV_CONTAINER_IMAGE` env var not fully wired. `main.rs` reads the env var and passes it to `GarageK8s`, but the fallback path in `garages.rs` and `DEFAULT_IMAGE` constant in `lib.rs` still hardcode `"ghcr.io/nhalm/moto-dev:latest"`.
- GET `/api/v1/wg/garages/{id}` returns dummy `peer_version` and `registered_at`. Handler hardcodes `peer_version: 0` and `registered_at: Utc::now()`. PostgreSQL values are never queried.

