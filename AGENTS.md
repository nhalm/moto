# Moto Agent Guidelines

## Spec Authority

**Specs are prescriptive, not descriptive.** The spec defines what code MUST do.

- **Spec is source of truth.** If code contradicts the spec, the code is wrong — refactor it.
- **Read the full spec on version changes.** When the spec version is newer than specd_history.md, re-read the entire spec — not just the changelog. The changelog summarizes what changed, but context lives in the full spec.
- **Don't build on broken foundations.** If existing code uses the wrong model (e.g., wrong ID scheme, wrong data flow), fix it first. Don't add new features on top of incorrect code.
- **Spec index:** `specs/README.md` lists all specifications organized by phase. Only specs with status "Ready to Rip" should be implemented.
- **Changelogs are immutable.** When creating new changelog entries, old entries never change.

## Loop System

The autonomous loop is defined in `loop.sh`. Commands live in `.claude/commands/`:

| Command          | Purpose                                                                        |
| ---------------- | ------------------------------------------------------------------------------ |
| `/specd:implement`     | Pick one unblocked work item, implement it, validate, record completion        |
| `/specd:audit`         | 3-phase spec-vs-code audit. Writes findings to specd_work_list.md and specd_review.md |
| `/specd:review-intake` | Process specd_review.md items into specd_work_list.md                                 |

## specd_work_list.md (Remaining Work)

The single execution queue for all work — spec implementations, audit findings, and promoted review items. **Read it in full** at the start of each iteration — it is kept small. Pick an unblocked item, implement it, then move it to specd_history.md.

## specd_history.md (Done Log)

specd_history.md is the archive of completed work in reverse chronological order (newest first). It does NOT contain remaining items — those live in specd_work_list.md.

Each entry is a single line: `- **spec-name v0.1 (YYYY-MM-DD):** description`. New entries go at the top of the file, below the header comment.

**Never read specd_history.md in full — it can get large.** Use `Grep` to search for specific specs or dates when checking for duplicates.

## specd_review.md (Human Decisions)

Ambiguous findings from audits that need human judgment. Items sit here until the human reviews them. On next loop start, `/specd:review-intake` promotes remaining items to specd_work_list.md (human deletes items they disagree with before restarting).

## Build & Test

**Always use `make` targets.** Never run `cargo`, `sqlx`, or other tooling directly. Run `make help` to see all available targets.

Common targets include `make test`, `make lint`, `make build`, `make check`, and `make ci` — but this is not an exhaustive list. Check `make help` for the current set of targets.

**Validate with `make test`, not just unit tests.** If integration tests fail, that's a real failure — do not dismiss them.

## Conventions

- All crates use the `moto-` prefix
- Follow patterns in existing code for naming, structure, and style
- See [specs/project-structure.md](specs/project-structure.md) for directory layout

### Test Organization

- **Tests belong in separate files.** For any module `foo.rs`, tests should be in `foo_test.rs` (same directory) or `tests/foo.rs` (integration tests).
- **Do not use `#[cfg(test)] mod tests {}` in source files.** This bloats source files and makes navigation harder.
- **Exception:** Small modules (<200 lines) may include inline tests if the tests are brief.

### Mocking and Traits

- **Use traits for external dependencies.** Database access, HTTP clients, K8s clients — anything that crosses a boundary.
- **Mock at boundaries, not internals.** Mock the trait, not the implementation details.
- **Handlers take trait objects.** This allows injecting mocks for testing without a database.
- **Integration tests use real implementations.** Only `*-db` crates hit real PostgreSQL.
