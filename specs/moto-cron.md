# Moto Cron

| | |
|--------|----------------------------------------------|
| Version | 0.4 |
| Status | Ripping |
| Last Updated | 2026-03-09 |

## Overview

TTL enforcement and scheduled cleanup for garages. Expired garages are terminated and their K8s namespaces deleted automatically.

**Design decision:** TTL enforcement runs inside moto-club's existing reconciliation loop, not as a separate K8s CronJob. The reconciler already has database and K8s client access, runs every 30 seconds, and handles garage lifecycle. Adding TTL checks there avoids a separate binary, container image, and deployment.

Future scheduled tasks (usage reports, resource audits) may warrant a standalone CronJob binary. That's deferred until the need arises.

## TTL Enforcement

### What happens when a garage expires

1. Reconciler calls `garage_repo::list_expired()` — returns garages where `expires_at < now()` and `status != terminated`
2. Take the first 10 results (rate limiting applied in the reconciler, not the query)
3. For each expired garage:
   a. Call `garage_repo::terminate(id, TtlExpired)` — sets `status = Terminated`, `terminated_at = now()`, `termination_reason = ttl_expired`
   b. Delete the K8s namespace `moto-garage-{short_id}` (same as user-initiated close)
   c. Log: `info garage_id={id} garage_name={name} reason=ttl_expired "garage expired, terminated"`
4. If namespace deletion fails after DB termination, log a warning and continue. The orphan cleanup step will catch leaked namespaces on subsequent cycles.
5. Continue to next expired garage (don't fail the whole batch if one fails)

### Scope

TTL enforcement applies to garages in **any non-terminated state**: Pending, Initializing, Ready, and Failed. A Failed garage that has exceeded its TTL should still be cleaned up.

### Properties

- **Safe from overwrites:** `terminate()` currently does an unconditional UPDATE. Add a `WHERE status != 'terminated'` guard so that a concurrent user-initiated close (with reason `user_closed`) is not overwritten by `ttl_expired`. The `list_expired()` query already filters terminated garages, but a race between user close and TTL enforcement is possible.
- **Crash-safe:** If the reconciler crashes mid-batch, the next cycle picks up remaining expired garages
- **Ordered:** Process oldest-expired first (`ORDER BY expires_at ASC`)
- **Rate-limited:** Process at most 10 expired garages per reconcile cycle to avoid thundering herd on mass expiry. Remaining garages are caught in the next cycle (30s later).

### Race with extend_ttl

A user could call `moto garage extend` at nearly the same moment the reconciler processes an expired garage. The API already rejects extensions on expired garages (410 GARAGE_EXPIRED), so the worst case is: garage expires, reconciler terminates it, and the extend call fails. This is acceptable — the user can open a new garage.

### Database support (already exists)

- `garage_repo::list_expired()` — queries `WHERE expires_at < now() AND status != 'terminated'`
- `garage_repo::terminate(id, reason)` — atomic UPDATE setting status, terminated_at, termination_reason (needs `WHERE status != 'terminated'` guard added)
- `TerminationReason::TtlExpired` enum variant
- Partial index `idx_garages_expires_at` on `garages(expires_at) WHERE status != 'terminated'`

### Integration point

The reconciler's `reconcile_once()` function gains a new step after existing K8s/DB sync:

```
Current steps:
1. Sync K8s namespace state → DB (pod status, missing pods, orphan cleanup)
2. Sync DB state → K8s (missing namespaces → mark terminated)

New step:
3. TTL enforcement — list_expired(limit 10) + terminate + delete namespace
```

### Audit log retention

The reconciler enforces retention on audit log tables. This runs as a new step after TTL enforcement:

```
4. Audit log retention — delete rows older than configured retention period
```

**Targets:**
- **moto-club** `audit_log` table: delete rows where `timestamp < now() - 30 days`
- **keybox** `audit_log` table: moto-club calls keybox's audit log cleanup endpoint (or keybox runs its own retention internally). Keybox retention is 90 days.

**Properties:**
- Runs once per reconcile cycle (every 30 seconds), but the DELETE is cheap when there's nothing to clean up
- Deletes in batches (at most 1000 rows per cycle) to avoid long-running transactions
- Logs: `info service=moto-club rows_deleted=42 retention_days=30 "audit log retention cleanup"`
- If deletion fails, log a warning and continue — retention is best-effort and will catch up on the next cycle

**Note:** Keybox manages its own retention (90-day period). moto-club does not reach into keybox's database directly. Keybox should add a similar retention step to its own startup or periodic task. See [audit-logging.md](audit-logging.md).

### Deferred: WireGuard session cleanup

moto-club.md mentions moto-cron cleaning up expired WireGuard session records. This is deferred to a future version — session cleanup is not blocking and can be addressed separately.

## TTL Warnings

The reconciler emits warning events before garage expiry via the event streaming WebSocket:

- 15 minutes before expiry
- 5 minutes before expiry

These are event stream messages, not blocking actions. The garage agent can call `moto garage extend` to prevent termination. Warnings are deduplicated per `(garage_id, threshold)` to avoid repeats across reconcile cycles. See [moto-club-websocket.md](moto-club-websocket.md).

## Configuration

No new env vars required. Uses existing:

| Variable | Default | Purpose |
|----------|---------|---------|
| `MOTO_CLUB_RECONCILE_INTERVAL_SECONDS` | `30` | How often the reconciler runs (including TTL checks) |

## References

- [moto-club.md](moto-club.md) — Reconciler architecture
- [garage-lifecycle.md](garage-lifecycle.md) — Garage state machine, termination
- [moto-club-websocket.md](moto-club-websocket.md) — TTL warning events (future)

## Changelog

### v0.4 (2026-03-09)
- Add audit log retention task to reconciler. Deletes moto-club audit rows older than 30 days. Keybox manages its own 90-day retention. Batched deletes (1000 per cycle), best-effort. See [audit-logging.md](audit-logging.md).

### v0.3 (2026-03-06)
- Fix: `terminate()` needs `WHERE status != 'terminated'` guard to prevent overwriting concurrent user-initiated close
- Clarify: rate limiting (10 per cycle) is applied in the reconciler, not in `list_expired()`
- Add: TTL enforcement applies to all non-terminated states including Failed
- Add: extend_ttl race condition acknowledged (acceptable behavior)
- Add: namespace deletion failure falls back to orphan cleanup
- Add: WireGuard session cleanup explicitly deferred

### v0.2 (2026-03-05)
- Full spec. TTL enforcement runs in moto-club reconciler, not a separate CronJob. Building blocks (list_expired, terminate, TtlExpired, partial index) already exist. Spec defines enforcement flow, ordering, rate limiting, and future TTL warnings.

### v0.1 (2026-01-21)
- Bare frame placeholder
