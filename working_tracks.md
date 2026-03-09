# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## audit-logging v0.2

- Ensure audit logging is best-effort: failures must not block primary operations
- Ensure sensitive data is never logged (secret values, API keys, tokens, request/response bodies)
- Add GET /api/v1/audit/logs endpoint on moto-club with query filters (service, event_type, principal_id, resource_type, since, until, limit, offset)
- Implement fan-out: moto-club queries own table and keybox /audit/logs in parallel, merges by timestamp, graceful degradation if keybox unreachable
- Auth: service token only for audit query endpoint
- Add audit log retention tasks to moto-cron reconciler (keybox 90 days, moto-club 30 days)

## audit-logging v0.3

- Add keybox GET /audit/logs endpoint for fan-out queries from moto-club (with query parameter pass-through)
