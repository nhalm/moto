# Moto Implementation Tracking

<!--
HOW TO USE THIS FILE:

1. Section header = "## spec-name.md vX.Y" - must match current spec version
2. If spec version > section version: check spec changelog, add new items to Remaining, update header
3. If no section exists: compare spec to code, create section with Implemented and Remaining lists
4. Pick ONE item from Remaining (skip blocked items)
5. Implement it, verify with tests
6. Move item from Remaining to Implemented
7. Code is the source of truth - if unsure whether something is implemented, check the code
-->

---

## project-structure.md v1.1

**Status:** Complete

**Implemented:**
- Cargo workspace with workspace dependencies
- rust-toolchain.toml pinning stable channel
- .cargo/config.toml with build settings and aliases
- Makefile with build/test/lint targets
- moto-common crate (Error enum, Result type, Secret<T> wrapper)
- moto-club-types crate (GarageId, GarageState, GarageInfo)
- moto-k8s crate (K8sClient, NamespaceOps trait, Labels)
- moto-garage crate (GarageMode, GarageClient)
- moto-cli crate (binary, clap parsing)

---

## moto-cli.md v0.2

**Status:** In Progress

**Implemented:**
- Global flags: --json/-j, --verbose/-v, --quiet/-q, --context/-c
- MOTO_JSON env var support
- Config file support (~/.config/moto/config.toml)
- Config: [output].color, [garage].ttl
- moto garage open: auto-generated names, --engine/-e, --ttl, --owner
- moto garage list: formatted output (NAME, STATUS, AGE, TTL, ENGINE)
- moto garage list: --context filter option (filter by kubectl context, "all" for all)
- moto garage close: accepts name, --force flag, confirmation prompt
- moto garage logs: --tail/-n, --since, --follow/-f streaming

**Remaining:**
- MOTOCONFIG env var (override kubeconfig)
- MOTO_NO_COLOR env var (disable colored output)
- Exit codes: differentiate 1 (general), 2 (not found), 3 (invalid input)
- garage enter (blocked: wgtunnel.md is Bare Frame)
- bike commands (blocked: bike.md is Wrenching)
- cluster commands (blocked: no spec)
