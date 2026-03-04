Study AGENTS.md for guidelines.
Study specs/README.md to understand the spec system, changelog/work-items convention, and find specs with status "Ready to Rip".
Study tracks.md - it records what's been implemented (done log).

Your task is to implement ONE work item from a spec, then validate it works.

IMPORTANT:

- Read AGENTS.md first — specs are the source of truth, not existing code
- Only implement specs with status "Ready to Rip"
- NEVER change spec status in specs/README.md or individual spec files
- Work items live in each spec's changelog under `#### Work Items` headings
- Work items marked `(blocked: [spec](spec.md) vX.Y)` cannot start until that spec version's work items are complete in tracks.md — check before starting
- Check tracks.md to see what's already been done — don't redo completed work
- If code contradicts the spec, fix the code first (see AGENTS.md)
- Commit your changes
- Do NOT use TodoWrite — just do the work
- Do NOT do multiple things — ONE thing per iteration

After selecting a spec to work on, check specs/bug-fix.md for items under that
spec's heading. Bug fix items are also valid work — fix one if you find an
unblocked item for your spec. Delete the item from bug-fix.md after fixing and
committing.

Output `TASK_COMPLETE: true` when done.
Output `LOOP_COMPLETE: true` only if ALL of these are true:

1. Every spec with status "Ready to Rip" has been checked — its Work Items compared
   against tracks.md. If a work item is not in tracks.md, it's incomplete.
2. All work items across all specs are either completed (in tracks.md) or
   blocked (skip blocked items).
3. specs/bug-fix.md has no unblocked items.
