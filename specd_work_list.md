# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## audit-logging v0.6

- Fix fan-out query logic in `crates/moto-club-api/src/audit.rs` to handle `offset` correctly: query each service with `offset+limit` rows (not forwarding offset), merge results by timestamp, then apply offset to the merged set (blocked: requires offset not to be forwarded to keybox in line 293-309)
- Add integration tests in `crates/moto-club-api/src/audit.rs` for offset parameter in fan-out queries to verify correct pagination across multiple services

