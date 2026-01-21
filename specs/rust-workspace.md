# Rust Workspace

| | |
|--------|----------------------------------------------|
| Status | Bare Frame |
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

Defines the Cargo workspace configuration, crate boundaries, shared dependencies, and how crates relate to each other.

## Jobs to Be Done

- [ ] Define workspace Cargo.toml structure
- [ ] Define initial crates and their purposes
- [ ] Define shared dependency versions
- [ ] Define crate dependency graph
- [ ] Define rust-toolchain.toml settings

## Specification

_To be written_

## Notes

Initial crates identified:
- `moto-cli` - Binary: arg parsing, command dispatch
- `moto-garage` - Library: garage (sandbox) lifecycle logic
- `moto-k3s` - Library: k3s/kubectl interactions
