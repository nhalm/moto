# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## audit-logging v0.4

- Fix moto-club garage audit events to use `principal_type: "service"` with `principal_id: "moto-club"` and add `"requested_by": username` to metadata for user-initiated operations (garage create/terminate), per spec requirement that `principal_id` must be SPIFFE ID or service name.

