# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## nix-removal v0.2

- Fix `specs/moto-bike.md` line 64: replace "Nix builds the base image" with Dockerfile reference
- Fix `specs/service-deploy.md` line 174: remove `mkBike` helper and "Nix build pipeline" reference, update to Docker build
- Fix `specs/README.md` line 52: change dev-container description from "Nix dockerTools container" to "Docker container"
- Fix `specs/dev-container.md` lines 92–93: change cargo-watch and cargo-nextest installation method from "apk" to "cargo install"
