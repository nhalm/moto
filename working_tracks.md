# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-club.md bug-fix

- Fallback name validation missing start/end alphanumeric + 63-char limit: `garages.rs` checks character set (lowercase + digits + hyphens) but doesn't enforce that name must start/end with alphanumeric or respect K8s 63-char label limit. Names like `-foo-` pass validation.

## keybox.md bug-fix

(all items completed)
