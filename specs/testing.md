# Testing Infrastructure

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Status | Ready to Rip |
| Last Updated | 2026-02-06 |

## Overview

Testing infrastructure for the Moto project. Integration tests require real PostgreSQL.

## Design Goals

- **Non-standard port**: Test database runs on port 5433 to avoid conflicts with dev database
- **Fresh database**: `make test-integration` tears down, starts up, migrates, then runs tests
- **Parallel execution**: Tests from different crates run in parallel
- **Isolation via UUIDs**: Tests use unique identifiers, not separate databases or transactions
- **All database crates**: Any crate that uses PostgreSQL should have integration tests

## Test Categories

**Unit tests** (`cargo test --lib`): No external dependencies. Pure functions, serialization, validation.

**Integration tests** (`cargo test --features integration`): Require PostgreSQL. Repository tests, API handler tests.

## Integration Feature Flag

Crates with database tests define an `integration` feature. Tests that need PostgreSQL are gated behind `#[cfg(feature = "integration")]`.

## moto-test-utils Crate

Shared test utilities for integration tests.

**Provides:**
- `test_pool()` - Returns connection to test database (port 5433)
- `unique_garage_name()` - Generates unique names for test isolation
- `unique_owner()` - Generates unique owner names
- `fake_wg_pubkey()` - Generates fake WireGuard public keys

**Design:**
- Single shared pool instance (not per-test)
- Panics with helpful message if database not running
- All `unique_*()` functions use UUIDs to guarantee no collisions

## Makefile

**Required targets:**
- `test-integration` - Ensures fresh database (teardown → startup → migrate all database crates → run tests)
- `test-all` - Runs unit tests and integration tests (with fresh database)
- `test-ci` - Runs unit tests and integration tests (for CI where database is already running)

**Migrations:** Run migrations for all `*-db` crates (moto-club-db, moto-keybox-db, etc.)

## CI

Integration tests run in CI with a PostgreSQL service container on port 5433.

## Changelog

### v0.3 (2026-02-06)
- Simplified spec to be directional, not prescriptive
- Clarified all database crates need integration tests
- Clarified migrations must run for all database crates

### v0.2 (2026-02-05)
- Added moto-test-utils crate specification
- Clarified test isolation strategy: unique identifiers, not separate databases

### v0.1 (2026-02-05)
- Initial spec
