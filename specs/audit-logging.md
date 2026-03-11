# Audit Logging

| | |
|--------|----------------------------------------------|
| Version | 0.6 |
| Status | Ready to Rip |
| Last Updated | 2026-03-09 |

## Overview

Platform-wide audit logging for moto. Standardizes what events are logged, how they're structured, and where they're stored. Builds on keybox's existing audit log (secret access events) and extends the pattern to all services.

**Key properties:**
- **Structured, canonical log lines** — one log line per request with all context accumulated
- **Append-only audit table** — tamper-evident storage in PostgreSQL
- **Per-service audit tables** — each service owns its audit data (no central aggregation service for v1)
- **Security events prioritized** — auth failures, secret access, garage lifecycle, AI API usage

**What this is NOT:**
- Not application logging (that's structured tracing via `tracing` crate)
- Not centralized log aggregation (no ELK/Loki — future)
- Not real-time alerting (future)

**Cross-spec impact:** Implementing this spec requires minor updates to:
- **keybox.md** — migrate existing `audit_log` table to the unified schema
- **moto-club.md** — add `GET /api/v1/audit/logs` query endpoint
- **moto-cron.md** — add audit log retention reconciliation tasks

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│  Each Service                                                    │
│                                                                   │
│  Request → Handler → AuditLogger::log(event) → audit_log table  │
│                                                                   │
│  Services write to their own database's audit_log table.         │
│  Schema is shared (same columns), but tables are independent.    │
└─────────────────────────────────────────────────────────────────┘

Query path:
  Admin → moto-club API → GET /api/v1/audit/logs → fan-out to services
                          (or direct keybox GET /audit/logs for secret events)
```

## Audit Event Schema

All services use the same audit event structure:

| Field | Type | Description |
|-------|------|-------------|
| `id` | UUID | Unique event ID |
| `event_type` | TEXT | Event category (see below) |
| `service` | TEXT | Which service produced the event (`keybox`, `moto-club`, `ai-proxy`) |
| `principal_type` | TEXT | `garage`, `bike`, `service`, or `anonymous` |
| `principal_id` | TEXT | SPIFFE ID or service name |
| `action` | TEXT | What happened (`create`, `read`, `delete`, `auth_fail`, etc.) |
| `resource_type` | TEXT | What was acted on (`secret`, `garage`, `ai_request`, etc.) |
| `resource_id` | TEXT | Identifier of the resource |
| `outcome` | TEXT | `success`, `denied`, `error` |
| `metadata` | JSONB | Service-specific additional context (no sensitive data) |
| `client_ip` | TEXT | Source IP from request headers or socket addr (if available) |
| `timestamp` | TIMESTAMPTZ | When it happened |

### Event types by service

**keybox:**

| Event Type | Action | Resource Type | When |
|-----------|--------|---------------|------|
| `secret_accessed` | `read` | `secret` | Secret value retrieved |
| `secret_created` | `create` | `secret` | New secret stored |
| `secret_updated` | `update` | `secret` | Secret value changed |
| `secret_deleted` | `delete` | `secret` | Secret removed |
| `dek_rotated` | `rotate` | `secret` | DEK rotation |
| `svid_issued` | `create` | `svid` | SVID issued for garage/bike |
| `auth_failed` | `auth_fail` | `token` | Invalid or expired token. `principal_type` MUST be `anonymous` (caller is unauthenticated). Failure reason goes in `metadata.reason`, not `resource_id`. |
| `access_denied` | `deny` | `secret` | ABAC policy denied access |

Keybox's existing `audit_log` table must be migrated to the unified schema. The current table has different column names (`spiffe_id`, `secret_scope`, `secret_name`) — the migration maps these to the new fields and adds missing columns (`service`, `action`, `resource_type`, `resource_id`, `outcome`, `metadata`, `client_ip`).

**moto-club:**

| Event Type | Action | Resource Type | When |
|-----------|--------|---------------|------|
| `garage_created` | `create` | `garage` | New garage provisioned |
| `garage_terminated` | `delete` | `garage` | Garage shut down |
| `garage_state_changed` | `update` | `garage` | State transition (e.g., Ready → Failed) |
| `ttl_enforced` | `delete` | `garage` | Garage terminated by TTL reconciler |
| `auth_failed` | `auth_fail` | `request` | Invalid auth on API request |

moto-club writes audit events from API handlers (user-initiated actions) and from the reconciler (TTL enforcement). If writing an audit event fails, the operation proceeds — audit logging is best-effort and must not block the primary operation.

**ai-proxy:**

| Event Type | Action | Resource Type | When |
|-----------|--------|---------------|------|
| `ai_request` | `proxy` | `ai_request` | AI API request forwarded to provider |
| `ai_request_denied` | `deny` | `ai_request` | Request blocked (auth, rate limit, unknown model) |
| `provider_error` | `error` | `ai_request` | Upstream provider returned error |

### What MUST NOT be logged

- Secret values (keybox already enforces this)
- API keys or tokens (real provider keys, garage SVIDs)
- Request/response bodies (AI prompts and completions contain user data)
- Password or credential material of any kind

### ai-proxy structured log format

Since ai-proxy is stateless (no database), it emits audit events as structured JSON log lines to stdout. Each line is a complete audit event matching the schema above:

```json
{"id":"550e8400-...","event_type":"ai_request","service":"ai-proxy","principal_type":"garage","principal_id":"spiffe://moto.local/garage/abc123","action":"proxy","resource_type":"ai_request","resource_id":"req-uuid","outcome":"success","metadata":{"provider":"anthropic","model":"claude-sonnet-4-20250514","mode":"passthrough","upstream_status":200,"duration_ms":1523},"client_ip":"10.42.0.15","timestamp":"2026-03-09T12:00:00Z"}
```

Token counts (`tokens_in`, `tokens_out`) are included in `metadata` when available from provider response headers. These are useful for usage tracking but are not sensitive.

A future log aggregation system can ingest these newline-delimited JSON lines.

## Storage

### Per-service audit tables

Each service maintains its own `audit_log` table in its database. The table schema matches the event schema above.

**keybox** already has an `audit_log` table (see keybox.md). It must be migrated to the unified schema (see keybox event types section above for details).

**moto-club** needs a new `audit_log` table in its database.

**ai-proxy** writes audit events to structured logs only (no database). This avoids adding database infrastructure to a stateless proxy. See "ai-proxy structured log format" above.

### Retention

| Service | Retention | Rationale |
|---------|-----------|-----------|
| keybox | 90 days | Secret access is security-critical |
| moto-club | 30 days | Garage lifecycle is operational |
| ai-proxy | Via log rotation | Structured log lines, rotated by infrastructure |

Retention for keybox and moto-club is enforced by moto-cron's reconciler (periodic deletion of rows older than the retention period). This requires adding audit log retention tasks to moto-cron.md — currently moto-cron only handles garage TTL enforcement and WireGuard cleanup.

### Indexes

```sql
CREATE INDEX idx_audit_log_timestamp ON audit_log(timestamp);
CREATE INDEX idx_audit_log_principal ON audit_log(principal_id);
CREATE INDEX idx_audit_log_event_type ON audit_log(event_type);
CREATE INDEX idx_audit_log_resource ON audit_log(resource_type, resource_id);
```

## Query API

### moto-club audit endpoint

```
GET /api/v1/audit/logs?service=keybox&event_type=secret_accessed&principal_id=garage-abc&since=2026-03-01T00:00:00Z&limit=100
```

**Auth:** Service token only. This endpoint is admin-only — garages and bikes cannot query audit logs.

**Query parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `service` | string | Filter by service (`keybox`, `moto-club`). If omitted, queries all services. |
| `event_type` | string | Filter by event type |
| `principal_id` | string | Filter by principal |
| `resource_type` | string | Filter by resource type |
| `since` | ISO 8601 | Events after this timestamp |
| `until` | ISO 8601 | Events before this timestamp |
| `limit` | integer | Max results (default 100, max 1000) |
| `offset` | integer | Pagination offset |

**Response:**

```json
{
  "events": [
    {
      "id": "...",
      "event_type": "secret_accessed",
      "service": "keybox",
      "principal_type": "garage",
      "principal_id": "spiffe://moto.local/garage/abc123",
      "action": "read",
      "resource_type": "secret",
      "resource_id": "global/ai/anthropic",
      "outcome": "success",
      "metadata": {},
      "timestamp": "2026-03-09T12:00:00Z"
    }
  ],
  "total": 42,
  "limit": 100,
  "offset": 0
}
```

### Fan-out behavior

When `service` is not specified, moto-club queries both its own audit table and keybox's `GET /audit/logs` endpoint in parallel, then merges results by timestamp (newest first).

**Query parameter pass-through:** All filter parameters (`event_type`, `principal_id`, `since`, `until`, etc.) are forwarded to keybox so filtering happens at the source.

**Pagination:** The `limit` and `offset` apply to the merged result set. Each service is queried with the full `limit` (offset is NOT forwarded) — results are merged, sorted by timestamp, then offset and truncated to `limit`. This ensures correct pagination: fetching each service with `offset+limit` rows, merging, then applying offset once to the merged set (not once per service).

**Error handling:** If keybox is unreachable, moto-club returns only its own events and includes a `"warnings": ["keybox unavailable"]` field in the response. The query does not fail.

**ai-proxy events** are not queryable via the API in v1 (log-only). This is noted in the response when `service=ai-proxy` is requested.

## Deferred Items

- **Central audit log aggregation** — unified storage (e.g., dedicated audit-db) instead of per-service tables
- **Log forwarding** — ship audit events to external SIEM/log system
- **Real-time alerting** — trigger alerts on suspicious patterns (e.g., rapid auth failures)
- **Tamper evidence** — hash chaining or signed log entries
- **ai-proxy database** — add persistent audit storage for ai-proxy (currently log-only)
- **CLI audit commands** — `moto audit search --principal garage-abc --since 24h`
- **Compliance reporting** — pre-built queries for SOC 2 / PCI DSS evidence
- **Per-event-type retention** — different retention periods for security vs. operational events

## References

- [keybox.md](keybox.md) — Existing audit log implementation (secret access events). Requires schema migration.
- [moto-club.md](moto-club.md) — Garage lifecycle events. Requires new audit query endpoint.
- [ai-proxy.md](ai-proxy.md) — AI request audit trail (structured logs only)
- [moto-cron.md](moto-cron.md) — Retention enforcement via reconciler. Requires new audit cleanup tasks.
- [compliance.md](compliance.md) — Future compliance requirements

## Changelog

### v0.6 (2026-03-11)
- Clarify fan-out pagination: `offset` is NOT forwarded to keybox. Instead, fan-out fetches `offset+limit` rows from each service, merges by timestamp, then applies offset to the merged set. This prevents double-skipping in paginated queries.

### v0.5 (2026-03-11)
- Clarify `auth_failed` events in keybox MUST use `principal_type: "anonymous"` (not "service") since the caller is unauthenticated.
- Clarify `auth_failed` failure reason belongs in `metadata.reason`, not in `resource_id` field.

### v0.4 (2026-03-11)
- Add `access_denied` event type for keybox ABAC policy denials (already implemented in code, missing from spec).

### v0.3 (2026-03-09)
- Add cross-spec impact callout: keybox.md, moto-club.md, moto-cron.md all need updates when this spec is implemented.
- Clarify keybox schema migration: current table has different column names, migration must map them to unified schema.
- Define ai-proxy structured log format: newline-delimited JSON to stdout matching the audit event schema.
- Clarify moto-club writes audit events from handlers and reconciler; audit logging is best-effort (must not block primary operations).
- Fix query API auth: service token only (not "admin SVID" which is undefined).
- Define fan-out behavior: parallel queries, parameter pass-through, merged results by timestamp, graceful degradation if keybox is unreachable.
- Add per-event-type retention to deferred items.
- Clarify `service` filter parameter: only `keybox` and `moto-club` are queryable in v1.

### v0.2 (2026-03-09)
- Full spec. Standardized audit event schema across services, per-service storage, query API.
- Define event types for keybox, moto-club, and ai-proxy.
- ai-proxy uses structured logs only for v1 (no database).
- Retention: keybox 90 days, moto-club 30 days, ai-proxy via log rotation.
- Query API on moto-club with fan-out to keybox.

### v0.1 (2026-01-19)
- Bare frame placeholder
