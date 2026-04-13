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

- [ ] Fix CI `build-images` job: club and keybox matrix jobs use `FROM moto-bike:latest` but run in parallel with the bike job on separate runners — `moto-bike:latest` is never available. Add a separate `build-bike` job and make club/keybox `needs: [build-bike]`, or build bike first and export the image as an artifact.
- [ ] Verify rustfmt and clippy installation in garage container: build Dockerfile.garage and test `cargo fmt --version && cargo clippy --version` inside. If they fail, add `rust-rustfmt` and `rust-clippy` apk packages. If they succeed, update dev-container.md spec table (lines 89-90) to document they are bundled with rust.
