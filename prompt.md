Study AGENTS.md for guidelines.
Study specs/README.md to understand the spec system and find specs with status "Ready to Rip".
Read working_tracks.md in full — it contains all remaining work items.

Your task is to implement ONE work item from working_tracks.md, then validate it works.

IMPORTANT:

- Read AGENTS.md first — specs are the source of truth, not existing code
- Pick an unblocked item from working_tracks.md — an item is unblocked if and only if its line does NOT contain the string `(blocked:`. Items without that string are ready to implement regardless of what version or section they're in.
- Read the full spec for context before implementing — the work item is a summary, the spec has the detail
- Only implement specs with status "Ready to Rip"
- NEVER change spec status in specs/README.md or individual spec files
- If code contradicts the spec, fix the code first (see AGENTS.md)
- Commit your changes
- Do NOT use TodoWrite — just do the work
- Do NOT do multiple things — ONE thing per iteration

After implementing, VALIDATE cross-file dependencies before recording completion:

- If you changed a trait or struct used across crates, verify all dependent crates still compile
- If you added/removed a K8s resource, verify the corresponding service.rs integration is updated
- If you modified API endpoints, verify CLI client calls match the new contract
- Run `make check` to catch compile errors and lint issues

After validation, update tracking files:

1. Move the completed item from working_tracks.md to tracks.md (under the matching `## spec vX.Y` section — create the section if it doesn't exist)
2. Check working_tracks.md for items with `(blocked: ...)` annotations that reference the work you just completed. If the blocker is resolved, remove the `(blocked: ...)` annotation.
   Output `TASK_COMPLETE: true` when done.

Before declaring LOOP_COMPLETE, re-read working_tracks.md and list every remaining
item. For each item, check: does the line contain `(blocked:`? If ANY item does
NOT contain `(blocked:`, you are NOT done — pick one and implement it.

Output `LOOP_COMPLETE: true` only if every remaining item in working_tracks.md
contains `(blocked:` on its line, or the file is empty.
