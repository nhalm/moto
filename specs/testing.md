# Testing Infrastructure

| | |
|--------|----------------------------------------------|
| Version | 0.4 |
| Status | Ready to Rip |
| Last Updated | 2026-02-06 |

## Overview

Testing infrastructure for the Moto project.

## Design Goals

- **Non-standard port**: Test database on port 5433 to avoid conflicts
- **Fresh database**: `make test-integration` tears down, starts up, migrates, runs tests
- **Parallel execution**: Tests from different crates run in parallel
- **Isolation via UUIDs**: Unique identifiers, not separate databases

## Test Architecture

**Database crates** (`*-db`): Integration tests that hit real PostgreSQL. Test repository functions directly.

**API/handler crates**: Unit tests with mocked database layer. Fast, no database needed.

This keeps integration tests focused on the database layer where they belong.

## moto-test-utils Crate

Shared utilities for integration tests: database pool, unique identifier generators.

## Makefile

- `test-integration` - Fresh database, run integration tests
- `test-all` - Unit tests + integration tests
- `test-ci` - For CI (database already running)

## Changelog

### v0.4 (2026-02-06)
- Added test architecture: db crates get integration tests, handlers use mocks

### v0.3 (2026-02-06)
- Simplified spec to be directional, not prescriptive
- Clarified all database crates need integration tests
- Clarified migrations must run for all database crates

### v0.2 (2026-02-05)
- Added moto-test-utils crate specification
- Clarified test isolation strategy: unique identifiers, not separate databases
- Added design goals section

### v0.1 (2026-02-05)
- Initial spec for test infrastructure
- Docker Compose for test PostgreSQL
- Makefile targets for test database management
- Integration test patterns with transaction rollback
- CI configuration examples
- Migration guide from in-memory stores
