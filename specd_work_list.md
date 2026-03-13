# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## docs v0.2

- Fix `docs/getting-started.md` port override env vars (line 279): change `CLUB_PORT=28080 KEYBOX_PORT=29090 AI_PROXY_PORT=27070` to use actual env var names `MOTO_CLUB_BIND_ADDR`, `MOTO_KEYBOX_BIND_ADDR`, `MOTO_AI_PROXY_BIND_ADDR` with full address values — the documented vars don't exist and are silently ignored.
