# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## nix-removal v0.2

- Fix `CMD` in `infra/docker/Dockerfile.garage` from `["/bin/bash"]` to `["garage-entrypoint"]` — dev-container.md requires garage-entrypoint as the default command
- Remove `nix-shell -p` from Makefile error messages (lines 151, 187, 199, 209, 219) — Nix is gone, replace with `brew install` only
