## Instructions

- study specs/README.md
- study tracks.md
- We are going to work on modifying and creating new specs.
- We do **not** write code unless explicitly asked to do so. We only modify \*.md files.
- When doing research use "model: Sonnet" agents in parallel to get the information that you need.

## Spec-vs-Code Analysis Workflow

Use a **team of agents** for audits. You (the team lead) coordinate three phases:

### Phase 1: Gather

Launch research agents in parallel to audit specs against code. Each agent reports raw findings back to you. Agents should not write or modify files.

### Phase 2: Validate

**Do not write findings directly to specs or bug-fix.md based on Phase 1 reports.** Agent research is frequently wrong about exact field names, log levels, parameter types, and other details.

Launch a second wave of validation agents. Each receives a batch of Phase 1 findings and:

1. Reads the actual code for each finding — confirms or rejects the claim against the source
2. Cross-checks against current bug-fix.md — if the finding was already reported and fixed (or intentionally skipped), marks it as duplicate
3. Categorizes each confirmed finding:
   - **Code is broken / clearly wrong per spec** → bug-fix.md candidate
   - **Code doesn't match spec but isn't actually broken** → flag for discussion (do not write anywhere yet)
   - **Spec is wrong or needs improvement** → spec update candidate (version bump required)
   - **Already known / intentionally deferred** → skip

### Phase 3: Write

Only after Phase 2 validation, write confirmed findings:

- **bug-fix.md:** Items where code is clearly doing something wrong that doesn't align with the spec. Use exact information (file paths, line numbers, field names) confirmed by validation agents.
- **Discussion items:** Present non-broken misalignments to the human for decision. The code may be fine and the spec may need updating, or vice versa — this requires human judgment.
- **Spec updates:** When the spec is wrong or incomplete and needs to reflect a better approach. Update the spec body with the WHAT — keep it prescriptive, not implementation-detailed. Bump the spec version. Put specifics of what needs to change in the changelog entry.

### Why this matters

Fresh agents have no memory of prior audit cycles. They will re-discover and re-report the same issues every time. Phase 2 validation prevents duplicate reports and inaccuracies. If you skip items without adding them to bug-fix.md, they will resurface in the next audit.
