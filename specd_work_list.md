# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## pre-commit v0.2 (compliance: content scanning)

- Add secret content scanning to `.githooks/pre-commit` using regex patterns on staged file contents (not just filenames). Scan for patterns: `sk-ant-`, `sk-proj-`, `sk-live-`, `AKIA`, `ghp_`, `gho_`, `xoxb-`, `xoxp-`, `-----BEGIN.*PRIVATE KEY-----`, base64-encoded key patterns. Block commit if found in staged diffs.

## service-deploy v0.7 (compliance: PodDisruptionBudgets)

- Create `infra/k8s/moto-system/pdb.yaml` with PodDisruptionBudgets for moto-keybox (`minAvailable: 2`) and moto-club (`minAvailable: 2`) — both run 3 replicas per bike.toml
- Add `pdb.yaml` to `infra/k8s/moto-system/kustomization.yaml` resources list

## audit-logging v0.6 (compliance: tamper-evident audit log)

- Create a SQL migration for moto-club-db that creates an `audit_writer` Postgres role with INSERT-only permission on the `audit_log` table (no UPDATE, no DELETE except via the retention function). Grant the application user this role for audit writes. The `delete_expired` retention function should use SECURITY DEFINER to run with elevated privileges.
- Create a matching SQL migration for moto-keybox-db with the same INSERT-only `audit_writer` role pattern on keybox's `audit_log` table

## moto-club v2.7 (compliance: leader election)

- Implement leader election for the reconciler using K8s Lease API in `crates/moto-club-reconcile/`. Create a `LeaderElector` that acquires/renews a Lease in the `moto-system` namespace. Only the leader runs `reconcile_once()`. Use 15s lease duration, 10s renew deadline, 2s retry period. On leadership loss, stop reconciling until re-elected.
- Add `leases` resource (`coordination.k8s.io` API group, verbs: `get, create, update`) to the moto-club ClusterRole in `infra/k8s/moto-system/club.yaml`

## makefile v0.20 (compliance: CI/CD pipeline)

- Create `.github/workflows/ci.yml` GitHub Actions workflow: trigger on push to main and PRs. Steps: checkout, install Nix, `make ci`, `make audit`. Use `ubuntu-latest` runner. Cache cargo registry and target dir.

## container-system v1.5 (compliance: image signing)

- Add Cosign image signing to the Nix build pipeline or Makefile: after `make push-*` targets, sign the image with `cosign sign`. Generate a cosign keypair stored in `.dev/cosign/` (gitignored). Add `make sign-images` target.
- (blocked: makefile v0.20 CI/CD — sign in CI after build)

