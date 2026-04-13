# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## nix-removal v0.3

- Fix `SSL_CERT_FILE` path in `specs/dev-container.md` lines 291 and 487: change `ca-bundle.crt` to `ca-certificates.crt` to match Wolfi base image
- Fix `SSL_CERT_FILE` path in `specs/container-system.md` line 191: change `ca-bundle.crt` to `ca-certificates.crt` to match Wolfi base image
