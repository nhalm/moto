# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-club-websocket v0.1

- Fix log streaming timestamps in `logs.rs`: timestamp is always `Utc::now()` (server clock) instead of the actual K8s pod log timestamp; should set `timestamps: true` in `PodLogOptions` and parse embedded timestamps

## compliance v0.4

- Investigate and remove `secrets` resource from moto-club ClusterRole (`infra/k8s/moto-system/club.yaml` lines 54-56): currently grants `secrets` with verbs [get, list, create, delete], violating least-privilege principle. Clarify whether this is needed for garage provisioning; if not, remove and verify functionality still works.

## moto-club v1.5

- Implement graceful shutdown timeout enforcement: apply `tokio::time::timeout(Duration::from_secs(30), ...)` around `axum::serve().with_graceful_shutdown()` call (`crates/moto-club/src/main.rs:69`). Constant `SHUTDOWN_GRACE_PERIOD_SECS` is defined but never enforced, leaving indefinite shutdown hangs possible.

## service-deploy v0.8

- Update `infra/k8s/moto-system/keybox.yaml` Service definition to explicitly expose port 9090 for Prometheus metrics (standard K8s pattern)
- Update `infra/k8s/moto-system/club.yaml` ClusterRole to explicitly document leases permission (coordination.k8s.io) for leader election among multiple replicas (standard K8s pattern)

