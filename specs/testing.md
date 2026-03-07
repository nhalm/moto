# Testing Infrastructure

| | |
|--------|----------------------------------------------|
| Version | 0.7 |
| Status | Ripping |
| Last Updated | 2026-03-07 |

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

## Smoke Tests

Smoke tests verify services work as deployed. They run against a live k3d cluster (`make deploy` must have succeeded).

**Convention:**
- Scripts live in `infra/smoke-test-{service}.sh`
- Each script assumes the service is reachable (via port-forward or cluster networking)
- Scripts exit 0 on success, 1 on failure
- Container image smoke tests (checking tools/config exist inside an image) remain as-is — they don't require k3d

**Makefile targets:**
- `smoke-keybox` — port-forwards keybox, runs `infra/smoke-test-keybox.sh`, cleans up
- `smoke-ai-proxy` — port-forwards ai-proxy, runs `infra/smoke-test-ai-proxy.sh`, cleans up

### Keybox Smoke Tests (`infra/smoke-test-keybox.sh`)

Requires: k3d cluster running with keybox deployed. Service token read from `.dev/k8s-secrets/service-token`.

**Auth matrix enforcement:**
- `POST /secrets/` with service token succeeds (200)
- `POST /secrets/` with SVID token returns 403 `FORBIDDEN`
- `DELETE /secrets/` with SVID token returns 403 `FORBIDDEN`
- `GET /secrets/` with service token succeeds (200)
- `GET /audit/logs` with service token succeeds (200)
- `GET /audit/logs` with SVID token returns 403 `FORBIDDEN`

**DEK rotation:**
- `POST /admin/rotate-dek/` with service token succeeds (200, version increments)
- `POST /admin/rotate-dek/` with SVID token returns 403 `FORBIDDEN`
- `POST /admin/rotate-dek/` for non-existent secret returns 404 `SECRET_NOT_FOUND`
- Secret value unchanged after rotation

**Cleanup:** Delete any secrets created during the test run.

### AI Proxy Smoke Tests (`infra/smoke-test-ai-proxy.sh`)

Requires: k3d cluster running with ai-proxy, keybox, and moto-club deployed. At least one AI provider key seeded in keybox (`ai-proxy/anthropic`).

**Passthrough route:**
- `POST /passthrough/anthropic/v1/messages` with valid SVID returns `200` (or upstream provider response)
- `POST /passthrough/anthropic/v1/messages` without auth returns `401`
- `POST /passthrough/anthropic/admin/billing` returns `403` (path allowlist enforcement)

**Unified endpoint:**
- `POST /v1/chat/completions` with `model: claude-sonnet-4-20250514` routes to Anthropic and returns `200`
- `POST /v1/chat/completions` with unknown model prefix returns `400`
- `POST /v1/chat/completions` without auth returns `401`

**Health endpoints:**
- `GET /health/live` returns `200`
- `GET /health/ready` returns `200`

**Provider key missing:**
- If `ai-proxy/openai` key is not seeded, a request for `gpt-4o` returns `503` with `provider not configured`

**Cleanup:** No persistent state created (ai-proxy is stateless).

## Changelog

### v0.7 (2026-03-07)
- Add ai-proxy smoke tests: passthrough auth/allowlist, unified endpoint routing, health endpoints, missing provider key.
- Add `smoke-ai-proxy` Makefile target.

### v0.6 (2026-03-02)
- Add smoke test section: convention for service-level smoke tests against k3d deployments.
- Define keybox smoke tests: auth matrix enforcement and DEK rotation against live cluster.

### v0.5 (2026-02-19)
- All repository functions should have not-found / error path tests (e.g., `update_secret_version` and `delete_secret` in secret_repo are missing not-found coverage)

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
