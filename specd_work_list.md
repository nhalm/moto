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

- Fix `principal_id` in reconciler audit events at `crates/moto-club-reconcile/src/garage.rs:502,754`: change from `"moto-club-reconciler"` to `"moto-club"` for `garage_state_changed` and `ttl_enforced` events. The spec requires `principal_id = "moto-club"` for service actions; reconciler context belongs in metadata.
- Add `garage_terminated` audit events for reconciler-driven terminations in `crates/moto-club-reconcile/src/garage.rs`: NamespaceMissing (line ~315), PodLost/Succeeded (line ~418), and PodLost/Unknown (line ~442) paths terminate garages without emitting audit events.

