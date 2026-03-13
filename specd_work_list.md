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

- Fix `docs/getting-started.md` line 179: change `ANTHROPIC_API_KEY="garage-abc123"` to use `$MOTO_GARAGE_SVID` — ai-proxy validates SVID JWTs, a bare garage ID string will return 401. The keybox curl on line 175 already uses `$MOTO_GARAGE_SVID` correctly.
