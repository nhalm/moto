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

- Fix `docs/security.md` line 270: remove link to `../specs/compliance.md` — docs must be self-contained with no links to `specs/`. Replace with inline summary of SOC 2 alignment or remove the reference.
- Remove or relocate `docs/garage-startup-steps.md` — internal engineering notes (bug writeups, workarounds, commit SHAs) that get published to the public GitHub Wiki via `cp -r docs/* wiki/`. Either move to a non-docs location (e.g. `notes/`) or add a `.wikiignore`/filter to the publish workflow.
