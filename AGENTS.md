# Moto Agent Guidelines

## Spec Authority

**Specs are prescriptive, not descriptive.** The spec defines what code MUST do.

- **Spec is source of truth.** If code contradicts the spec, the code is wrong — refactor it.
- **Read the full spec on version changes.** When the spec version is newer than tracks.md, re-read the entire spec — not just the changelog. The changelog summarizes what changed, but context lives in the full spec.
- **Don't build on broken foundations.** If existing code uses the wrong model (e.g., wrong ID scheme, wrong data flow), fix it first. Don't add new features on top of incorrect code.
- **Spec index:** `specs/README.md` lists all specifications organized by phase. Only specs with status "Ready to Rip" should be implemented.
- **Changelogs are immutable.** When creating new changelog entries, old entries never change.

## tracks.md (Done Log)

tracks.md records what's been implemented. It does NOT contain "Remaining" lists — the spec's Work Items are the source of truth for what needs doing.

**Never read tracks.md in full — it exceeds context limits.** Use targeted reads:

1. **Find your section:** `Grep` for `## <your-spec>` to get the line number
2. **Read your section:** `Read` with `offset` and `limit` (typically 30-50 lines) starting from that line number
3. **Write updates:** Use `Edit` to modify only your section — never rewrite the file
4. **After completing a work item:** Add it to the Implemented list under the matching spec version section

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
