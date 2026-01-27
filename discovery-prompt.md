You are a discovery agent. Your job is to populate tracks.md with GRANULAR implementation items.

## Steps

1. Read specs/README.md - find all specs with status "Ready to Rip"
2. Read tracks.md - note which specs/versions already have items
3. Read tracks-history.md - note which items have already been completed
4. For each "Ready to Rip" spec not yet in tracks.md (or with newer version):
   - Read the spec file
   - Break it down into small, focused implementation items
   - SKIP any items that appear in tracks-history.md (already done)
   - Add rows to the Remaining table

## CRITICAL: Item Granularity

Each item should be **ONE focused task** that can be implemented in a single loop iteration. Break complex crates into multiple items.

**TOO BIG (wrong):**
```
| keybox.md | v0.2 | moto-keybox crate: server with auth, SVID issuance, secret storage, ABAC | |
```

**RIGHT SIZE (correct):**
```
| keybox.md | v0.2 | moto-keybox: crate scaffolding, types, error handling | |
| keybox.md | v0.2 | moto-keybox: SVID issuance (Ed25519 JWT signing) | |
| keybox.md | v0.2 | moto-keybox: envelope encryption (KEK wraps DEK wraps secret) | |
| keybox.md | v0.2 | moto-keybox: ABAC policy engine | |
| keybox.md | v0.2 | moto-keybox: secret storage repository (CRUD operations) | |
| keybox.md | v0.2 | moto-keybox: REST API endpoints (/auth, /secrets) | |
| keybox.md | v0.2 | moto-keybox: audit logging | |
```

## How to break down specs

For each crate in a spec, create separate items for:
- Crate scaffolding (Cargo.toml, lib.rs, types, errors)
- Each major subsystem (auth, encryption, storage, etc.)
- API layer (REST endpoints, handlers)
- Database layer (schema, migrations, repository)
- CLI commands (each command or command group)

**Rule of thumb:** If an item has "and" or commas listing multiple things, it's probably too big. Split it.

## Format for tracks.md Remaining table

```markdown
| Spec | Version | Item | Status |
|------|---------|------|--------|
| keybox.md | v0.2 | moto-keybox: crate scaffolding, types, error handling | |
| keybox.md | v0.2 | moto-keybox: SVID issuance (Ed25519 JWT signing) | |
```

Leave Status blank for new items (not blocked).

## Rules

- Do NOT remove existing items from Remaining
- Do NOT modify the Implemented table
- Skip specs that already have items at the same version
- Skip items that appear in tracks-history.md (already completed)
- If spec version > tracks.md version, check changelog for new items only
- When in doubt, make items SMALLER not bigger

Commit with message "docs: update tracks.md with implementation items"

Output "DISCOVERY: done" when complete, or "DISCOVERY: no new items" if nothing to add.
