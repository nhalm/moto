# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## audit-logging v0.3

- Parallelize audit fan-out: use `tokio::join!` to query local audit_log and keybox `/audit/logs` concurrently in `crates/moto-club-api/src/audit.rs`
- Extract `tokens_in`/`tokens_out` from provider response headers into ai-proxy audit event metadata in `crates/moto-ai-proxy/src/audit.rs`
- Add keybox 90-day audit log retention task to moto-cron reconciler (batch delete keybox audit rows older than 90 days, same pattern as moto-club 30-day retention)


