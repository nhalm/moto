# Nix Removal

| | |
|--------|----------------------------------------------|
| Version | 0.3 |
| Status | Ripping |
| Last Updated | 2026-04-13 |

## Changelog

### v0.3 (2026-04-13)
- Add `moto-bike.md` to "Specs to Update" list — Build Pipeline section still prescribes `nix build` commands

### v0.2 (2026-04-10)
- Rewrite: remove implementation details (Dockerfiles, code examples). Spec defines WHAT and WHY, not HOW.
- Change base image from Debian to Wolfi (Chainguard) for minimal CVE footprint

### v0.1 (2026-04-10)
- Initial spec (draft)

## Overview

Remove Nix entirely from the Moto build system. Replace all Nix-based container builds with standard Dockerfiles using Wolfi (Chainguard) as the base image.

**Why:**
- Nix adds significant build time — CI has no binary cache, so every run fetches the full Nix store cold
- The persistent `nix-store` Docker volume consumes large amounts of disk locally
- The Docker-wrapped Nix pattern (running `nix build` inside a `nixos/nix` container) adds indirection and complexity
- Reproducibility via Nix is not meaningful for ephemeral dev containers or scratch-based production images where `Cargo.lock` already pins Rust dependencies
- Standard Dockerfiles are universally understood by developers and AI agents

**Scope:**
- Delete all Nix files (flake.nix, flake.lock, infra/modules/, infra/pkgs/)
- Create Dockerfiles for all container images (garage, bike, club, keybox)
- Update Makefile build targets
- Update CI workflow
- Update all specs that reference Nix

**Out of scope:**
- Changing what tools are in the garage (same tools, different build method)
- Changing the container security model
- Changing the registry, signing, or push workflow
- Local Rust development (already Nix-free)
- Garage customization / per-team toolchains (future spec)

## Specification

### Base Image: Wolfi (Chainguard)

All Moto containers that need a base image use `cgr.dev/chainguard/wolfi-base`.

**Why Wolfi:**
- Minimal CVE footprint — packages rebuilt daily by Chainguard
- glibc-based — no musl compatibility issues with Rust, openssl, or libpq
- Small base (~15MB)
- Uses `apk` package manager

### Garage Container (`moto-garage`)

The garage image is a single-stage build containing the full dev toolchain. It must include every tool listed in the "Included Tooling" section of dev-container.md. The contents of the garage do not change — only the build method changes from Nix dockerTools to a Dockerfile.

**Dockerfile location:** `infra/docker/Dockerfile.garage`

**Package availability:** Most tools are in Wolfi's apk repos. For tools not available (e.g., jujutsu, ttyd, yq), install from official release binaries.

### Bike Base Image (`moto-bike`)

The bike remains a `FROM scratch` image containing only CA certificates, timezone data, and a non-root user. A Wolfi stage extracts these artifacts.

**Dockerfile location:** `infra/docker/Dockerfile.bike`

### Engine Images (club, keybox)

Each engine image is a multi-stage Dockerfile: a Wolfi-based builder stage compiles the Rust binary, then copies it onto the `moto-bike` base. This replaces crane.

Docker layer caching of the dependency build step replaces crane's `buildDepsOnly`.

**Dockerfile locations:**
- `infra/docker/Dockerfile.club`
- `infra/docker/Dockerfile.keybox`

### Infrastructure Directory Structure

```
moto/
├── rust-toolchain.toml              # Pins Rust version (already exists)
├── .cargo/config.toml               # Cargo settings (already exists)
└── infra/
    ├── docker/
    │   ├── Dockerfile.garage
    │   ├── Dockerfile.bike
    │   ├── Dockerfile.club
    │   └── Dockerfile.keybox
    └── smoke-test.sh                # Unchanged
```

### Makefile

- All `build-*` targets use `docker build` instead of Docker-wrapped Nix
- Remove `NIX_LINUX_SYSTEM` variable
- Remove `clean-nix-cache` target
- `push-*`, `sign-*`, `scan-*` targets unchanged

### CI

- Remove `DeterminateSystems/nix-installer-action` from `.github/workflows/ci.yml`
- `build-images` job uses `docker build` directly
- Add Docker layer caching (buildx)

### Files to Delete

All Nix files:
- `flake.nix`, `flake.lock`
- `infra/pkgs/` — `default.nix`, `moto-garage.nix`, `moto-bike.nix`, `moto-club.nix`, `moto-keybox.nix`
- `infra/modules/` — `base.nix`, `dev-tools.nix`, `terminal.nix`, `wireguard.nix`

### Specs to Update

Remove Nix references and update to reflect Dockerfile approach:
- `dev-container.md` — major rewrite (build philosophy, structure, build commands)
- `container-system.md` — build pipeline, directory structure
- `makefile.md` — prerequisites, build targets
- `project-structure.md` — directory layout
- `moto-bike.md` — Build Pipeline section, bike.toml note (replace Nix flake references with Dockerfile approach)
- `local-dev.md`, `pre-commit.md`, `garage-isolation.md`, `docs.md` — minor reference cleanup

## References

- [dev-container.md](dev-container.md) — Garage container spec (to be updated)
- [container-system.md](container-system.md) — Build pipeline spec (to be updated)
- [moto-bike.md](moto-bike.md) — Bike base image spec (to be updated)
- [Wolfi](https://github.com/wolfi-dev) — Chainguard's minimal container OS
