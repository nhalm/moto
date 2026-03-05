# Bug Fix Punch List

Living punch list for cross-cutting code bugs, wiring omissions, and small
fixes that don't warrant a spec version bump. Items are grouped by owning spec.

Loop agents: when implementing a spec, check this file for items under that
spec's heading. Fix one per iteration. Delete the item from this file after
fixing and committing.

Items marked `(blocked: ...)` can't be fixed until their dependency resolves —
same convention as tracks.md.

---

## project-structure.md

(none)

## moto-cli.md

(none)

## jj-workflow.md

(none)

## pre-commit.md

(none)

## makefile.md

- `make run` target (Makefile line 48) uses `cargo run --bin moto-cli` but the binary is named `moto` in `crates/moto-cli/Cargo.toml` line 12. Other targets (`install`, `dev`, `dev-cluster`) correctly use `--bin moto`. Fix: change to `cargo run --bin moto`.

## testing.md

- Garage container smoke test is `infra/smoke-test.sh` but spec naming convention (line 43) says `infra/smoke-test-{service}.sh`. Rename to `infra/smoke-test-garage.sh` and update Makefile `test-garage` target to match.

## moto-club.md

(none)

## keybox.md

(none)

## dev-container.md

(none)

## container-system.md

- Missing `[profile.release]` section in root `Cargo.toml`. Spec (lines 927-931) requires `lto = true`, `codegen-units = 1`, `strip = true` for bike container size optimization.
- `Cargo.toml` line 11 has `rust-version = "1.85"` but `flake.nix` line 21 pins `1.88.0`. Update `rust-version` to `"1.88"` to match the actual toolchain.

## local-cluster.md

(none)

## garage-isolation.md

- NetworkPolicy keybox egress rule (`crates/moto-club-k8s/src/network_policy.rs` line 144) uses pod selector `app: keybox`, but keybox pods (`infra/k8s/moto-system/keybox.yaml` line 46) have label `app.kubernetes.io/component: moto-keybox`. The selector never matches, so garage pods cannot reach keybox through the NetworkPolicy. Fix: align the pod selector label to match the actual keybox pod labels.

## garage-lifecycle.md

- CLI `garage open` missing `--name` flag. Spec (line 82) defines `--name <name>` but CLI (`crates/moto-cli/src/commands/garage.rs` GarageAction::Open) always auto-generates the name. The backend `CreateGarageInput.name` already accepts `Option<String>`. Add `--name` CLI arg and pass it through.
- CLI `garage open` missing `--image` flag. Spec (line 86) defines `--image <image>` to override the dev container image but CLI always passes `image: None`. The backend `CreateGarageInput.image` already accepts `Option<String>`. Add `--image` CLI arg and pass it through.

## moto-bike.md

- Deployment builder `build_env_vars()` (`crates/moto-k8s/src/deployment.rs` lines 558-588) only injects `POD_NAME`, `POD_NAMESPACE`, `RUST_LOG`. Missing `RUST_BACKTRACE="1"` which spec (line 112) lists as a common env var for all engines.

## supporting-services.md

(none)

## moto-wgtunnel.md

(none)

## local-dev.md

(none)

## service-deploy.md

- `club.yaml` has no security contexts. `keybox.yaml` has both pod-level (`runAsUser: 1000`, `runAsGroup: 1000`, `runAsNonRoot: true`) and container-level (`readOnlyRootFilesystem: true`, `allowPrivilegeEscalation: false`, `capabilities: drop: [ALL]`). Add matching security contexts to `club.yaml`.
- `keybox.yaml` missing metrics port 9090. `club.yaml` has it in both Service (port 9090) and container ports (`containerPort: 9090`). Spec (moto-bike.md line 168) says all engines expose Prometheus metrics on port 9090. Add port 9090 to keybox Service and container port list.
