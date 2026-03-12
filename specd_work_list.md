# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-throttle v0.1

- Fix `client_ip()` to fall back to socket address instead of `"unknown"` (`crates/moto-throttle/src/layer.rs:290-297`): spec requires key = client IP from X-Forwarded-For or socket addr; all clients without the header currently share one bucket

## ai-proxy v1.3

- Fix `/health/ready` to check keybox reachability and key availability: currently only checks `is_startup_complete()` flag; spec requires "keybox reachable, at least one provider key cached"; `has_cached_keys()` exists but is not wired in
- Fix `/health/startup` to verify initial key fetch complete: `mark_startup_complete()` is called after SVID fetch but before any provider API key is fetched; spec requires "SVID loaded, initial key fetch complete"

## audit-logging v0.1

- Fix `garage_created` and `garage_terminated` audit events to log requesting user as principal (`crates/moto-club-api/src/garages.rs:882-883`): currently logs `principal_type: "service"` / `principal_id: "moto-club"` instead of the actual user; user identity (`owner`) is available but only put in metadata

## moto-club-websocket v0.1

- Fix event subscriber cleanup race in `events.rs`: uses `subscriber_count(&owner) <= 1` to decide when to remove an owner's channel, but the departing handler's receiver is still in scope — a second connected subscriber's channel gets deleted, causing connection loss; should check `== 0` after dropping the receiver
- Fix log streaming timestamps in `logs.rs`: timestamp is always `Utc::now()` (server clock) instead of the actual K8s pod log timestamp; should set `timestamps: true` in `PodLogOptions` and parse embedded timestamps

## compliance v0.4

- Investigate and remove `secrets` resource from moto-club ClusterRole (`infra/k8s/moto-system/club.yaml` lines 54-56): currently grants `secrets` with verbs [get, list, create, delete], violating least-privilege principle. Clarify whether this is needed for garage provisioning; if not, remove and verify functionality still works.

## moto-club v1.5

- Implement graceful shutdown timeout enforcement: apply `tokio::time::timeout(Duration::from_secs(30), ...)` around `axum::serve().with_graceful_shutdown()` call (`crates/moto-club/src/main.rs:69`). Constant `SHUTDOWN_GRACE_PERIOD_SECS` is defined but never enforced, leaving indefinite shutdown hangs possible.

## service-deploy v0.8

- Update `infra/k8s/moto-system/keybox.yaml` Service definition to explicitly expose port 9090 for Prometheus metrics (standard K8s pattern)
- Update `infra/k8s/moto-system/club.yaml` ClusterRole to explicitly document leases permission (coordination.k8s.io) for leader election among multiple replicas (standard K8s pattern)

