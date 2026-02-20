Study AGENTS.md for guidelines.
Study specs/README.md to find specs with status "Ready to Rip".
Study tracks.md - it has instructions and tracks what's Implemented vs Remaining.

Your task is to implement ONE item from a "Remaining" list, then validate it works.

IMPORTANT:

- Read AGENTS.md first - specs are the source of truth, not existing code
- Only implement specs with status "Ready to Rip"
- NEVER change spec status in specs/README.md or individual spec files
- Follow the instructions in tracks.md
- If code contradicts the spec, fix the code first (see AGENTS.md)
- Check the spec Changelog for recent changes that might affect existing code
- Do NOT use TodoWrite - just do the work
- Do NOT do multiple things - ONE thing per iteration

AFTER IMPLEMENTING - you MUST do these before committing:

1. Update tracks.md: move the item you implemented from **Remaining** to **Implemented**
2. If the spec version in tracks.md doesn't match the spec file, update the section header
3. Commit your code changes AND the tracks.md update together in one commit

Output `TASK_COMPLETE: true` when done.
Output `LOOP_COMPLETE: true` if "Remaining" is empty (only blocked items left).
