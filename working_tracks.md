# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-club.md bug-fix

(all items completed)

## container-system.md bug-fix

(all items completed)

## moto-bike.md bug-fix

(all items completed)

## keybox.md bug-fix

(all items completed)

## makefile.md bug-fix

(all items completed)

## testing.md bug-fix

(all items completed)

## container-system.md bug-fix (2)

- Add `[profile.release]` section to root `Cargo.toml` with `lto = true`, `codegen-units = 1`, `strip = true` per spec.
- Update `Cargo.toml` `rust-version` from `"1.85"` to `"1.88"` to match `flake.nix` toolchain pin.

## garage-isolation.md bug-fix

- Fix NetworkPolicy keybox egress rule pod selector from `app: keybox` to `app.kubernetes.io/component: moto-keybox` to match actual keybox pod labels.

## garage-lifecycle.md bug-fix

- Add `--name` CLI arg to `garage open` command and pass it through to `CreateGarageInput.name`.
- Add `--image` CLI arg to `garage open` command and pass it through to `CreateGarageInput.image`.

## moto-bike.md bug-fix (2)

- Add `RUST_BACKTRACE="1"` to deployment builder `build_env_vars()` per spec.

## service-deploy.md bug-fix

- Add security contexts to `club.yaml` matching `keybox.yaml` (runAsUser/runAsGroup/runAsNonRoot pod-level, readOnlyRootFilesystem/allowPrivilegeEscalation/capabilities container-level).
- Add metrics port 9090 to `keybox.yaml` Service and container port list per moto-bike.md spec.
- Replace static manifests with deployment builder usage or generate from bike.toml per moto-bike.md v0.6 changelog.
