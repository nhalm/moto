# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## nix-removal v0.3

- Fix `infra/docker/Dockerfile.club` dep-cache layer: the `cargo build --release --bin moto-club` step fails because `moto-club-api` and `moto-club-db` declare `moto-test-utils` as a `[dev-dependencies]` entry, and Cargo resolves dev-dep path deps at workspace load time. The Dockerfile does not copy `crates/moto-test-utils/Cargo.toml` into the builder stage, so Cargo errors with "failed to load manifest for dependency `moto-test-utils`". Add `COPY crates/moto-test-utils/Cargo.toml crates/moto-test-utils/` and a corresponding stub `crates/moto-test-utils/src/lib.rs` alongside the other stubs (verified with a local reproducer using the real workspace manifest).
- Fix `infra/docker/Dockerfile.keybox` dep-cache layer: same root cause — `moto-keybox-db` has `moto-test-utils` as a `[dev-dependencies]` entry, and `crates/moto-test-utils/Cargo.toml` is not copied into the builder stage, so `cargo build --release --bin moto-keybox-server` fails at workspace load. Add `COPY crates/moto-test-utils/Cargo.toml crates/moto-test-utils/` plus a stub `src/lib.rs`.
- Validate the fixes by running `make build-club && make build-keybox` end-to-end on a clean cache.
