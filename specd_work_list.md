# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## nix-removal v0.3

- Implement separate Rust dependency caching layer in Dockerfile.club and Dockerfile.keybox: add a build-dependencies-only step using stub src/main.rs (per container-system.md pattern lines 515-528) before copying source code, to enable Docker layer caching of dependencies

