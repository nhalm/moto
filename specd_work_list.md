# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to specd_history.md
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## keybox v0.16 (compliance CRITICAL-1)

- Add service token authentication to `POST /auth/token` in `crates/moto-keybox/src/api.rs` and `pg_api.rs` — only moto-club (via service token) should be able to issue SVIDs. Return 401 for unauthenticated callers.

## ai-proxy v0.5 (compliance CRITICAL-2)

- Verify Ed25519 SVID signature in `crates/moto-ai-proxy/src/auth.rs` `extract_garage_id()` — load keybox's public verifying key at startup, verify signature before trusting claims. A forged JWT MUST be rejected.

## garage-isolation v0.5 (compliance HIGH-1)

- Add IPv6 NetworkPolicy rules in `crates/moto-club-k8s/src/network_policy.rs` — mirror all IPv4 egress rules with IPv6 equivalents. Block `fd00::/8` (ULA/WireGuard overlay), `::1/128` (loopback), `fe80::/10` (link-local).

## keybox v0.16 (compliance HIGH-2)

- Restrict garage access to service-scoped secrets in `crates/moto-keybox/src/abac.rs` `evaluate_service()` — garages should NOT be able to read `ai-proxy/*` secrets directly. Add a deny-list for sensitive service prefixes or require explicit grant. (blocked: ai-proxy v0.5 CRITICAL-2 — fix ai-proxy auth first so garages use ai-proxy instead of direct keybox)

## service-deploy v0.7 (compliance HIGH-3)

- Scope moto-club K8s RBAC: replace cluster-wide `secrets` permission with namespace-scoped access. moto-club should NOT be able to read secrets in `moto-system`. Options: create per-garage Roles dynamically, or exclude `moto-system` namespace.

## garage-isolation v0.5 (compliance HIGH-4)

- Add `automount_service_account_token: Some(false)` to postgres and redis pod specs in `crates/moto-club-k8s/src/supporting_services.rs` `build_postgres_deployment()` and `build_redis_deployment()`

## audit-logging v0.3

- Parallelize audit fan-out: use `tokio::join!` to query local audit_log and keybox `/audit/logs` concurrently in `crates/moto-club-api/src/audit.rs`
- Extract `tokens_in`/`tokens_out` from provider response headers into ai-proxy audit event metadata in `crates/moto-ai-proxy/src/audit.rs`
- Add keybox 90-day audit log retention task to moto-cron reconciler (batch delete keybox audit rows older than 90 days, same pattern as moto-club 30-day retention)


