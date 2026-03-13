# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## docs v0.1

- Fix `docs/deployment.md` ai-proxy replica count: change 3 to 2 (lines 34, 216) — `bike.toml` and K8s manifest specify `replicas: 2`
