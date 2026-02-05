# Testing Infrastructure

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Status | Ready to Rip |
| Last Updated | 2026-02-05 |

## Overview

This spec defines the testing infrastructure for the Moto project. With the removal of in-memory stores, integration tests require real PostgreSQL. This spec covers:

- Test database setup via Docker Compose
- Integration test patterns
- Makefile targets for running tests
- CI considerations

## Prerequisites

Before running integration tests, ensure you have:

1. **Docker** (Docker Desktop or Docker Engine)
   - Verify: `docker --version`

2. **Docker Compose** (v2)
   - Verify: `docker compose version`

3. **sqlx-cli** for running migrations
   ```bash
   cargo install sqlx-cli --no-default-features --features postgres
   ```
   - Verify: `sqlx --version`

4. **Port 5433 available** (test database port)
   - Check: `lsof -i :5433` (should return nothing)

## Test Dependencies

### Docker Compose for Test Infrastructure

```yaml
# docker-compose.test.yml
services:
  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_USER: moto_test
      POSTGRES_PASSWORD: moto_test
      POSTGRES_DB: moto_test
    ports:
      - "5433:5432"  # Different port to avoid conflicts with dev database
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U moto_test"]
      interval: 2s
      timeout: 5s
      retries: 10
```

### Test Database URL

```bash
# For integration tests
TEST_DATABASE_URL="postgres://moto_test:moto_test@localhost:5433/moto_test"
```

## Test Categories

### Unit Tests

Unit tests that don't require external dependencies:
- Pure function tests
- Serialization/deserialization tests
- Type conversion tests
- Error handling tests

```bash
# Run unit tests only (no database required)
cargo test --lib
```

### Integration Tests

Tests that require PostgreSQL:
- Repository tests (CRUD operations)
- Service layer tests
- API handler tests with real database
- Migration tests

```bash
# Run integration tests (requires test database)
cargo test --features integration
```

### Feature Flag

Use a Cargo feature flag to separate integration tests:

```toml
# Cargo.toml
[features]
default = []
integration = []
```

```rust
// In test files
#[cfg(feature = "integration")]
mod integration_tests {
    // Tests that require PostgreSQL
}
```

## Makefile Targets

### Test Targets

```makefile
# Test database URL (override with environment variable if needed)
TEST_DATABASE_URL ?= postgres://moto_test:moto_test@localhost:5433/moto_test

.PHONY: test test-db-up test-db-down test-db-migrate test-db-reset test-integration test-all test-ci

# Run unit tests only (fast, no dependencies)
test:
	cargo test --lib

# Start test database
test-db-up:
	docker compose -f docker-compose.test.yml up -d
	@echo "Waiting for PostgreSQL..."
	@until docker compose -f docker-compose.test.yml exec -T postgres pg_isready -U moto_test; do sleep 1; done
	@echo "PostgreSQL is ready"

# Stop test database
test-db-down:
	docker compose -f docker-compose.test.yml down -v

# Run database migrations on test database
test-db-migrate:
	DATABASE_URL=$(TEST_DATABASE_URL) sqlx migrate run --source crates/moto-club-db/migrations

# Reset test database (drop and recreate)
test-db-reset: test-db-down test-db-up test-db-migrate

# Run integration tests (starts database if needed)
test-integration: test-db-up test-db-migrate
	@TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test --features integration || \
		(echo "Tests failed, cleaning up..." && $(MAKE) test-db-down && exit 1)
	@$(MAKE) test-db-down

# Run all tests
test-all: test test-integration

# CI target (assumes database is already running)
test-ci:
	cargo test --lib
	TEST_DATABASE_URL=$(TEST_DATABASE_URL) cargo test --features integration
```

## Test Database Management

### Setup Pattern

Each integration test should:
1. Use a transaction that rolls back (for isolation)
2. Or use a unique schema/database per test

**Recommended: Transaction rollback pattern**

```rust
#[cfg(feature = "integration")]
mod tests {
    use sqlx::PgPool;

    async fn test_pool() -> PgPool {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for integration tests");
        PgPool::connect(&url).await.expect("Failed to connect to test database")
    }

    #[tokio::test]
    async fn test_create_garage() {
        let pool = test_pool().await;
        let mut tx = pool.begin().await.unwrap();

        // Test code using &mut tx instead of &pool
        // ...

        // Transaction automatically rolls back when dropped (no commit)
    }
}
```

### Shared Test Fixtures

Create a test utilities crate or module:

```
crates/
  moto-test-utils/
    src/
      lib.rs        # Test database pool, fixtures
      fixtures.rs   # Common test data
```

## CI Configuration

### GitHub Actions Example

```yaml
# .github/workflows/test.yml
name: Tests

on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --lib

  integration-tests:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16-alpine
        env:
          POSTGRES_USER: moto_test
          POSTGRES_PASSWORD: moto_test
          POSTGRES_DB: moto_test
        ports:
          - 5433:5432
        options: >-
          --health-cmd "pg_isready -U moto_test"
          --health-interval 2s
          --health-timeout 5s
          --health-retries 10
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Run migrations
        run: |
          cargo install sqlx-cli --no-default-features --features postgres
          DATABASE_URL=postgres://moto_test:moto_test@localhost:5433/moto_test sqlx migrate run --source crates/moto-club-db/migrations
      - name: Run integration tests
        env:
          TEST_DATABASE_URL: postgres://moto_test:moto_test@localhost:5433/moto_test
        run: cargo test --features integration
```

## Migration from In-Memory Stores

Tests that previously used in-memory stores should be converted:

### Before (in-memory)
```rust
#[test]
fn test_peer_registration() {
    let ipam_store = InMemoryStore::new(...);
    let peer_store = InMemoryPeerStore::new();
    let registry = PeerRegistry::new(peer_store, Ipam::new(ipam_store));
    // ...
}
```

### After (integration test)
```rust
#[cfg(feature = "integration")]
#[tokio::test]
async fn test_peer_registration() {
    let pool = test_pool().await;

    // Use PostgreSQL stores with the test database pool
    let ipam_store = PostgresIpamStore::new(pool.clone());
    let peer_store = PostgresPeerStore::new(pool.clone());
    let registry = PeerRegistry::new(peer_store, Ipam::new(ipam_store));
    // ...

    // Note: For test isolation, either:
    // 1. Use unique identifiers (UUIDs) to avoid conflicts
    // 2. Clean up test data after each test
    // 3. Run integration tests serially (--test-threads=1)
}
```

**Test isolation strategies:**

Since the PostgreSQL stores take a `DbPool` (not a transaction), choose one of these isolation approaches:

1. **Unique identifiers** (recommended): Generate unique test data per test using UUIDs
2. **Cleanup after test**: Delete test data in a cleanup block or `Drop` impl
3. **Serial execution**: Run integration tests with `cargo test --features integration -- --test-threads=1`
4. **Database reset**: Use `make test-db-reset` between test runs during development

### Tests That Should Remain Unit Tests

Some tests don't need a database:
- Serialization tests (JSON, TOML parsing)
- Type conversion tests
- Validation logic tests
- Error type tests
- Pure business logic without I/O

Keep these as regular `#[test]` without the `integration` feature.

## Local Development Workflow

```bash
# One-time setup
make test-db-up
make test-db-migrate

# Run tests during development
cargo test --lib                    # Fast unit tests
cargo test --features integration   # Integration tests

# Cleanup
make test-db-down
```

## Changelog

### v0.1 (2026-02-05)
- Initial spec for test infrastructure
- Docker Compose for test PostgreSQL
- Makefile targets for test database management
- Integration test patterns with transaction rollback
- CI configuration examples
- Migration guide from in-memory stores
