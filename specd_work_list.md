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

- Run `make build-club && make build-keybox` to validate engine images build correctly
- Update spec `dev-container.md` — replace Nix dockerTools with Dockerfile approach, update philosophy, structure, build sections
- Update spec `container-system.md` — replace build pipeline diagram, directory structure, Nix references
- Update spec `makefile.md` — remove Nix prerequisites, update build target docs, remove `clean-nix-cache`
- Update spec `project-structure.md` — replace `infra/modules/` and `infra/pkgs/` with `infra/docker/`, remove `flake.nix`/`flake.lock`
- Update specs with minor Nix references: `local-dev.md`, `pre-commit.md`, `garage-isolation.md`, `docs.md`
