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

- Fix `docs/ai-proxy.md` line 107: change "public key from moto-club" to "public key from keybox" — ai-proxy fetches the verifying key from keybox (`GET {keybox_url}/auth/verifying-key`), not from moto-club.
- Fix `docs/architecture.md` line 194: change "using fake API key `garage-{id}`" to reference the SVID JWT — garages use their SVID JWT as the API key value, not a plain `garage-{id}` string. A bare garage ID would fail SVID signature verification.
