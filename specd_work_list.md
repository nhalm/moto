# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## makefile v0.20 (compliance: CI/CD pipeline)

- Create `.github/workflows/ci.yml` GitHub Actions workflow: trigger on push to main and PRs. Steps: checkout, install Nix, `make ci`, `make audit`. Use `ubuntu-latest` runner. Cache cargo registry and target dir.

## container-system v1.5 (compliance: image signing)

- Add Cosign image signing to the Nix build pipeline or Makefile: after `make push-*` targets, sign the image with `cosign sign`. Generate a cosign keypair stored in `.dev/cosign/` (gitignored). Add `make sign-images` target.
- (blocked: makefile v0.20 CI/CD — sign in CI after build)

