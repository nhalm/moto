# Moto Agent Guidelines

## Spec Authority

**Specs are prescriptive, not descriptive.** The spec defines what code MUST do.

- **Spec is source of truth.** If code contradicts the spec, the code is wrong - refactor it.
- **Read the Changelog.** Before implementing, check the spec's Changelog for recent changes. If a changelog entry describes behavior that existing code violates, that's a refactoring task.
- **Don't build on broken foundations.** If existing code uses the wrong model (e.g., wrong ID scheme, wrong data flow), fix it first. Don't add new features on top of incorrect code.
- **Spec index:** `specs/README.md` lists all specifications organized by phase.
- **Changelogs are immutable.** When creating new changelog entries, old entries never change.

## Conventions

- All crates use the `moto-` prefix
- Follow patterns in existing code for naming, structure, and style
- See [specs/project-structure.md](specs/project-structure.md) for directory layout

### Test Organization

- **Tests belong in separate files.** For any module `foo.rs`, tests should be in `foo_test.rs` (same directory) or `tests/foo.rs` (integration tests).
- **Do not use `#[cfg(test)] mod tests {}` in source files.** This bloats source files and makes navigation harder.
- **Exception:** Small modules (<200 lines) may include inline tests if the tests are brief.

### Mocking and Traits

- **Use traits for external dependencies.** Database access, HTTP clients, K8s clients - anything that crosses a boundary.
- **Mock at boundaries, not internals.** Mock the trait, not the implementation details.
- **Handlers take trait objects.** This allows injecting mocks for testing without a database.
- **Integration tests use real implementations.** Only `*-db` crates hit real PostgreSQL.
