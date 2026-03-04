Study AGENTS.md for guidelines.
Study specs/README.md to find all specs with status "Ready to Rip".

Your task is to populate working_tracks.md with work items that haven't been completed yet.

PROCESS:

1. Read specs/README.md to find every spec with status "Ready to Rip".
2. For each Ready to Rip spec, read the full spec file and extract ALL Work Items from every changelog version.
3. Read tracks.md to find completed items. Use targeted reads — grep for `## spec-name` to find the right section, then read that section. NEVER read tracks.md in full.
4. Read working_tracks.md to find items already queued.
5. Read specs/bug-fix.md for any pending bug fix items.
6. For each work item from step 2 that is NOT recorded in tracks.md AND NOT already in working_tracks.md, add it to working_tracks.md under the correct `## spec-name vX.Y` section header.

RULES:

- Preserve existing content in working_tracks.md — only ADD new items, never remove or rewrite existing items
- Use the exact format from working_tracks.md: section headers are `## spec-name vX.Y`, items are `- description`
- If a work item depends on another item that hasn't been completed yet, add `(blocked: dependency description)` at the end of the line
- Only include items from specs with status "Ready to Rip" — skip Bare Frame, Wrenching, and Ripping specs
- Include bug-fix.md items that are not yet resolved (not deleted from the file)
- Do NOT implement anything — only populate the tracking file

Output `SEED_COMPLETE: true` when done.
