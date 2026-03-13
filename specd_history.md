# Moto Implementation Tracking

<!--
This file is a DONE LOG — it records what has been implemented, not what remains.
Remaining work lives in specd_work_list.md.

HOW TO USE THIS FILE:

1. Section header = "## spec-name vX.Y" — must match the spec version you're working on
2. When you complete a work item, move it from specd_work_list.md to the Implemented list under the matching section
3. If no section exists for the current spec version, create one at the TOP (below these instructions)
4. Never read this file in full — it exceeds context limits. Use Grep to find your section, then Read with offset/limit.

WHAT GOES HERE:
- Work items moved from specd_work_list.md after completion
- Bug-fix.md items, recorded after completion

WHAT DOES NOT GO HERE:
- Remaining work — that lives in specd_work_list.md
- Discovery or TODO items — those belong in specd_work_list.md or bug-fix.md
-->

---

- **docs v0.2 (2026-03-13):** Fix `docs/ai-proxy.md` KEYBOX_URL default (line 153): change `http://keybox.moto-system:8080` to `http://moto-keybox.moto-system:8080` — must match actual K8s service name in `ai-proxy.yaml` line 89.
- **docs v0.2 (2026-03-13):** Fix `docs/getting-started.md` in-cluster DNS hostnames (lines 176, 180): change `keybox.moto-system.svc.cluster.local` to `moto-keybox.moto-system.svc.cluster.local` and `ai-proxy.moto-system.svc.cluster.local` to `moto-ai-proxy.moto-system.svc.cluster.local` — K8s services are named `moto-keybox` and `moto-ai-proxy` per infra/k8s manifests.
- **docs v0.2 (2026-03-13):** Fix `docs/getting-started.md` local dev ports: change keybox from `:19090` to `:8090` and ai-proxy from `:17070` to `:18090` (lines 92-98, 110, 113, 276) — these are the `moto dev up` output and health check URLs. Code defaults are `0.0.0.0:8090` (keybox) and `0.0.0.0:18090` (ai-proxy) per `dev.rs` lines 37, 45.
- **docs v0.2 (2026-03-13):** Fix `docs/architecture.md` line 321: remove references to internal spec filenames (`moto-cron.md`, `moto-club-websocket.md`) — docs must be self-contained with no references to `specs/`. Replace with generic phrasing like "planned for future implementation".
- **docs v0.2 (2026-03-13):** Fix `docs/security.md` lines 70 and 211: change moto-club port from 18080 to 8080 — 18080 is the `kubectl port-forward` host-side binding, not the in-cluster service port. NetworkPolicy operates on in-cluster ports; the K8s Service (`club.yaml`) exposes port 8080.
- **docs v0.2 (2026-03-13):** Fix `docs/architecture.md` line 194: change "using fake API key `garage-{id}`" to reference the SVID JWT — garages use their SVID JWT as the API key value, not a plain `garage-{id}` string. A bare garage ID would fail SVID signature verification.
- **docs v0.2 (2026-03-13):** Fix `docs/ai-proxy.md` line 107: change "public key from moto-club" to "public key from keybox" — ai-proxy fetches the verifying key from keybox (`GET {keybox_url}/auth/verifying-key`), not from moto-club.
- **docs v0.2 (2026-03-13):** Fix `docs/ai-proxy.md` line 111: change "Invalid or expired SVIDs return `401 Unauthorized`" — expired SVIDs actually return `403 Forbidden` (same as non-ready garages). Only missing/malformed tokens return 401. Update to reflect the code's distinction: missing/invalid → 401, expired/not-garage/not-ready → 403.
- **docs v0.2 (2026-03-13):** Fix `docs/getting-started.md` line 179: change `ANTHROPIC_API_KEY="garage-abc123"` to use `$MOTO_GARAGE_SVID` — ai-proxy validates SVID JWTs, a bare garage ID string will return 401. The keybox curl on line 175 already uses `$MOTO_GARAGE_SVID` correctly.
- **docs v0.2 (2026-03-13):** Fix `docs/deployment.md` line 257: remove link to `specs/compliance.md` — docs must be self-contained with no `specs/` links (will break on wiki publish). Replace with inline summary or remove the reference.
- **docs v0.2 (2026-03-13):** Remove or relocate `docs/garage-startup-steps.md` — internal engineering notes (bug writeups, workarounds, commit SHAs) that get published to the public GitHub Wiki via `cp -r docs/* wiki/`. Either move to a non-docs location (e.g. `notes/`) or add a `.wikiignore`/filter to the publish workflow.
- **docs v0.2 (2026-03-13):** Fix `docs/security.md` line 270: remove link to `../specs/compliance.md` — docs must be self-contained with no links to `specs/`. Replace with inline summary of SOC 2 alignment or remove the reference.
- **docs v0.2 (2026-03-13):** Fix `docs/architecture.md` line 307: change "3-replica Deployments for moto-club, keybox, and ai-proxy" to reflect that ai-proxy is 2 replicas (moto-club and keybox are 3)
- **docs v0.1 (2026-03-13):** Fix `docs/deployment.md` ai-proxy replica count: change 3 to 2 (lines 34, 216) — `bike.toml` and K8s manifest specify `replicas: 2`
- **docs v0.1 (2026-03-13):** Fix `docs/getting-started.md` keybox in-cluster port: change `:9090` to `:8080` in curl example (line 176) — keybox API is on port 8080, not 9090
- **docs v0.1 (2026-03-13):** Fix `docs/security.md` keybox egress port: change 9090 to 8080 (lines 66, 207) — port 9090 is metrics, API is on 8080 per `keybox.yaml` Service definition
- **docs v0.1 (2026-03-13):** Fix `docs/getting-started.md` ai-proxy in-cluster port: change `:7070` to `:8080` in curl example (line 180) — ai-proxy listens on 8080 per `bike.toml` and K8s manifest
- **docs v0.1 (2026-03-13):** Fix `docs/getting-started.md` registry port: change `localhost:5555` to `localhost:5050` (lines 73, 77, 86) — all other docs and k3d config use 5050
- **docs v0.1 (2026-03-13):** Fix `docs/components.md` keybox anti-enumeration status code: change 404 to 403 (line 66) — code returns 403 for both "not found" and "access denied" (`moto-keybox/src/api.rs` line 621)
- **docs v0.1 (2026-03-13):** Fix `docs/security.md` SPIFFE trust domain: change `moto.internal` to `moto.local` (lines 133, 134, 139) — must match code (`moto-keybox/src/types.rs` TRUST_DOMAIN) and other docs
- **docs v0.1 (2026-03-13):** Write `.github/workflows/wiki-publish.yml` — publish `docs/` to GitHub Wiki on push to main
- **docs v0.1 (2026-03-13):** Write `README.md` — project landing page: tagline, what Moto is, how-it-works diagram, doc links
- **docs v0.1 (2026-03-13):** Write `docs/components.md` — reference table and short sections for each component
- **docs v0.1 (2026-03-13):** Write `docs/ai-proxy.md` — the problem, how it works, passthrough vs unified, security, configuration
- **docs v0.1 (2026-03-13):** Write `docs/security.md` — threat model, isolation layers, SPIFFE SVIDs, keybox encryption, network boundaries, compliance
- **docs v0.1 (2026-03-13):** Write `docs/deployment.md` — `make deploy`, what runs where, secrets, port-forward, production considerations
- **docs v0.1 (2026-03-13):** Write `docs/getting-started.md` — prerequisites, `moto dev up` walkthrough, first garage, stopping
- **docs v0.1 (2026-03-13):** Write `docs/architecture.md` — component map, design philosophy, data flow, motorcycle metaphor glossary
- **service-deploy v0.8 (2026-03-11):** Update `infra/k8s/moto-system/club.yaml` ClusterRole to explicitly document leases permission (coordination.k8s.io) for leader election among multiple replicas (standard K8s pattern)
- **service-deploy v0.8 (2026-03-11):** Update `infra/k8s/moto-system/keybox.yaml` Service definition to explicitly expose port 9090 for Prometheus metrics (standard K8s pattern)
- **moto-club v1.5 (2026-03-11):** Implement graceful shutdown timeout enforcement: apply `tokio::time::timeout(Duration::from_secs(30), ...)` around `axum::serve().with_graceful_shutdown()` call (`crates/moto-club/src/main.rs:69`). Constant `SHUTDOWN_GRACE_PERIOD_SECS` is defined but never enforced, leaving indefinite shutdown hangs possible.
- **compliance v0.4 (2026-03-11):** Investigate and remove `secrets` resource from moto-club ClusterRole (`infra/k8s/moto-system/club.yaml` lines 54-56): currently grants `secrets` with verbs [get, list, create, delete], violating least-privilege principle. Clarify whether this is needed for garage provisioning; if not, remove and verify functionality still works.
- **moto-club-websocket v0.1 (2026-03-11):** Fix log streaming timestamps in `logs.rs`: timestamp is always `Utc::now()` (server clock) instead of the actual K8s pod log timestamp; should set `timestamps: true` in `PodLogOptions` and parse embedded timestamps
- **moto-club-websocket v0.1 (2026-03-11):** Fix event subscriber cleanup race in `events.rs`: uses `subscriber_count(&owner) <= 1` to decide when to remove an owner's channel, but the departing handler's receiver is still in scope — a second connected subscriber's channel gets deleted, causing connection loss; should check `== 0` after dropping the receiver
- **audit-logging v0.1 (2026-03-11):** Fix `garage_created` and `garage_terminated` audit events to log requesting user as principal (`crates/moto-club-api/src/garages.rs:882-883`): currently logs `principal_type: "service"` / `principal_id: "moto-club"` instead of the actual user; user identity (`owner`) is available but only put in metadata
- **ai-proxy v1.3 (2026-03-11):** Fix `/health/startup` to verify initial key fetch complete: `mark_startup_complete()` is called after SVID fetch but before any provider API key is fetched; spec requires "SVID loaded, initial key fetch complete"
- **ai-proxy v1.3 (2026-03-11):** Fix `/health/ready` to check keybox reachability and key availability: currently only checks `is_startup_complete()` flag; spec requires "keybox reachable, at least one provider key cached"; `has_cached_keys()` exists but is not wired in
- **moto-throttle v0.1 (2026-03-11):** Fix `client_ip()` to fall back to socket address instead of `"unknown"` (`crates/moto-throttle/src/layer.rs:290-297`): spec requires key = client IP from X-Forwarded-For or socket addr; all clients without the header currently share one bucket
- **moto-club (WG) v1.5 (2026-03-11):** Add ownership check to `PeerRegistry::register_device` for re-registration (`crates/moto-club-wg/src/peers.rs:212-218`): when an existing device is found by public key, code returns it unconditionally regardless of owner — spec requires 403 DEVICE_NOT_OWNED if owner differs
- **keybox v1.5 (2026-03-11):** Fix in-memory `AuditEntryResponse::from` to preserve `metadata` and `client_ip` fields (`crates/moto-keybox/src/api.rs:362-363`): hardcodes empty `{}` metadata and `None` client_ip, dropping actual data
- **keybox v1.5 (2026-03-11):** Fix `validate_service_token` to return 403 instead of 500 when service token is not configured (`crates/moto-keybox/src/api.rs:576-583`): spec requires 403 for auth failures, not 500
- **garage-lifecycle v0.3 (2026-03-11):** Implement unsaved changes warning on `garage close`: spec requires checking for unsaved changes and prompting to sync first; code only does a generic Y/N prompt
- **moto-cli v1.5 (2026-03-11):** Fix `garage extend` human-readable output to use `format_expires_at()` helper for `expires_at` display (`crates/moto-cli/src/commands/garage.rs:897`): currently prints raw RFC 3339 string (e.g., `2026-01-20T04:48:00Z`) instead of formatted display
- **garage-lifecycle v0.3 (2026-03-11):** Fix `garage close` order: code terminates DB record before deleting K8s namespace, but spec requires namespace deletion first then DB update (`crates/moto-club-garage/src/service.rs`)
- **garage-lifecycle v0.3 (2026-03-11):** Fix `is_terminal()` in `crates/moto-club-garage/src/lifecycle.rs`: incorrectly marks `Ready` as a terminal state. `Ready` is an active operational state; only `Failed` and `Terminated` are terminal per the spec state machine
- **container-system v1.5 (2026-03-11):** Sign images in CI after build (add signing step to `.github/workflows/ci.yml` after image builds)
- **container-system v1.5 (2026-03-11):** Add Cosign image signing to the Nix build pipeline or Makefile: after `make push-*` targets, sign the image with `cosign sign`. Generate a cosign keypair stored in `.dev/cosign/` (gitignored). Add `make sign-images` target.
- **makefile v0.20 (2026-03-11):** Create `.github/workflows/ci.yml` GitHub Actions workflow: trigger on push to main and PRs. Steps: checkout, install Nix, `make ci`, `make audit`. Use `ubuntu-latest` runner. Cache cargo registry and target dir.
- **audit-logging v0.6 (2026-03-11):** Create a matching SQL migration for moto-keybox-db with the same INSERT-only `audit_writer` role pattern on keybox's `audit_log` table
- **moto-club v2.7 (2026-03-11):** Add `leases` resource (`coordination.k8s.io` API group, verbs: `get, create, update`) to the moto-club ClusterRole in `infra/k8s/moto-system/club.yaml`
- **moto-club v2.7 (2026-03-11):** Implement leader election for the reconciler using K8s Lease API in `crates/moto-club-reconcile/`. Create a `LeaderElector` that acquires/renews a Lease in the `moto-system` namespace. Only the leader runs `reconcile_once()`. Use 15s lease duration, 10s renew deadline, 2s retry period. On leadership loss, stop reconciling until re-elected.
- **audit-logging v0.6 (2026-03-11):** Create a SQL migration for moto-club-db that creates an `audit_writer` Postgres role with INSERT-only permission on the `audit_log` table (no UPDATE, no DELETE except via the retention function). Grant the application user this role for audit writes. The `delete_expired` retention function should use SECURITY DEFINER to run with elevated privileges.
- **service-deploy v0.7 (2026-03-11):** Create `infra/k8s/moto-system/pdb.yaml` with PodDisruptionBudgets for moto-keybox (`minAvailable: 2`) and moto-club (`minAvailable: 2`) — both run 3 replicas per bike.toml. Add `pdb.yaml` to `infra/k8s/moto-system/kustomization.yaml` resources list.
- **pre-commit v0.2 (2026-03-11):** Add secret content scanning to `.githooks/pre-commit` using regex patterns on staged file contents (not just filenames). Scan for patterns: `sk-ant-`, `sk-proj-`, `sk-live-`, `AKIA`, `ghp_`, `gho_`, `xoxb-`, `xoxp-`, `-----BEGIN.*PRIVATE KEY-----`, base64-encoded key patterns. Block commit if found in staged diffs.
- **makefile v0.20 (2026-03-11):** Add `cargo install cargo-audit` to dev setup if not present, and add `make audit` target that runs `cargo audit` to check for known CVEs in dependencies. Add `cargo audit` to the `ci` target so it runs as part of `make ci`.
- **audit-logging v0.6 (2026-03-11):** Add `garage_terminated` audit events for reconciler-driven terminations in `crates/moto-club-reconcile/src/garage.rs`: NamespaceMissing (line ~315), PodLost/Succeeded (line ~418), and PodLost/Unknown (line ~442) paths terminate garages without emitting audit events.
- **audit-logging v0.6 (2026-03-11):** Fix `principal_id` in reconciler audit events at `crates/moto-club-reconcile/src/garage.rs:502,754`: change from `"moto-club-reconciler"` to `"moto-club"` for `garage_state_changed` and `ttl_enforced` events. The spec requires `principal_id = "moto-club"` for service actions; reconciler context belongs in metadata.
- **audit-logging v0.6 (2026-03-11):** Fix `outcome` field in `PgSecretRepository::audit()` at `crates/moto-keybox/src/pg_repository.rs:759`: currently hardcoded to `"success"` for all event types. `AccessDenied` events must use `outcome = "denied"`. Determine outcome from event type instead of hardcoding.
- **audit-logging v0.6 (2026-03-11):** Emit `access_denied` audit events from `PgSecretRepository` when ABAC policy denies access. Currently, `policy.evaluate()` and `policy.can_read()` return `Err(AccessDenied)` which propagates via `?` before any audit call. Each ABAC check in `create_with_context`, `get_with_context`, `update_with_context`, and `delete_with_context` must catch the error, log the `access_denied` audit event, then re-return the error. File: `crates/moto-keybox/src/pg_repository.rs`
- **audit-logging v0.6 (2026-03-11):** Fix `access_denied` action value from `"auth_fail"` to `"deny"` in `audit_fields_for_event()` at `crates/moto-keybox/src/pg_repository.rs:851` and in `AuditEntry::access_denied()` at `crates/moto-keybox/src/types.rs:384`
- **audit-logging v0.6 (2026-03-11):** Fix `AuditEntry::auth_failed()` helper at `crates/moto-keybox/src/types.rs:362-373`: change `principal_type` from `PrincipalType::Service` to `PrincipalType::Anonymous`, and move `reason` from `resource_id` to `metadata` field (add metadata field to `AuditEntry` struct if needed). Update corresponding test at line 553.
- **audit-logging v0.6 (2026-03-11):** Fix fan-out query logic in `crates/moto-club-api/src/audit.rs` to handle `offset` correctly: query each service with `offset+limit` rows (not forwarding offset), merge results by timestamp, then apply offset to the merged set
- **audit-logging v0.6 (2026-03-11):** Add integration tests in `crates/moto-club-api/src/audit.rs` for offset parameter in fan-out queries to verify correct pagination across multiple services
- **audit-logging v0.5 (2026-03-11):** Fix keybox `audit_auth_failed` in `crates/moto-keybox/src/pg_api.rs` to store the failure reason in `metadata` (e.g. `{"reason": "..."}`) instead of `resource_id`; set `resource_id` to empty string or omit
- **audit-logging v0.5 (2026-03-11):** Fix keybox `audit_auth_failed` in `crates/moto-keybox/src/pg_api.rs` to use `DbPrincipalType::Anonymous` instead of `DbPrincipalType::Service` for auth failure events
- **audit-logging v0.5 (2026-03-11):** Add `Anonymous` variant to keybox `PrincipalType` enum in `crates/moto-keybox-db/src/models.rs` and update `Display` impl; update `from_db_principal_type` in `crates/moto-keybox/src/pg_api.rs` to map the new variant
- **audit-logging v0.4 (2026-03-11):** Fix moto-club garage audit events to use `principal_type: "service"` with `principal_id: "moto-club"` and add `"requested_by": username` to metadata for user-initiated operations (garage create/terminate), per spec requirement that `principal_id` must be SPIFFE ID or service name.
- **audit-logging v0.4 (2026-03-11):** Add keybox 90-day audit retention to moto-cron reconciler: `moto-keybox-db::audit_repo::delete_expired` exists but nothing calls it. Add a reconciler step that calls it with 90-day retention (previously marked complete in error).
- **audit-logging v0.4 (2026-03-11):** Add `limit` and `offset` fields to keybox `AuditLogsResponse` in `crates/moto-keybox/src/api.rs` so direct keybox audit queries support proper pagination.
- **audit-logging v0.4 (2026-03-11):** Fix silent event loss in keybox fan-out: when timestamp parsing fails in `crates/moto-club-api/src/audit.rs` `filter_map`, events are silently dropped but `total` still counts them, breaking pagination. Either log and skip with adjusted total, or propagate parse errors as warnings.
- **audit-logging v0.3 (2026-03-11):** Add keybox 90-day audit log retention task to moto-cron reconciler (batch delete keybox audit rows older than 90 days, same pattern as moto-club 30-day retention)
- **audit-logging v0.3 (2026-03-11):** Extract `tokens_in`/`tokens_out` from provider response headers into ai-proxy audit event metadata in `crates/moto-ai-proxy/src/audit.rs`
- **audit-logging v0.3 (2026-03-11):** Parallelize audit fan-out: use `tokio::join!` to query local audit_log and keybox `/audit/logs` concurrently in `crates/moto-club-api/src/audit.rs`
- **service-deploy v0.7 (2026-03-11):** Scope moto-club K8s RBAC: replace cluster-wide `secrets` permission with namespace-scoped access. moto-club should NOT be able to read secrets in `moto-system`. Options: create per-garage Roles dynamically, or exclude `moto-system` namespace.
- **garage-isolation v0.5 (2026-03-11):** Add `automount_service_account_token: Some(false)` to postgres and redis pod specs in `crates/moto-club-k8s/src/supporting_services.rs` `build_postgres_deployment()` and `build_redis_deployment()`
- **keybox v0.16 (2026-03-11):** Restrict garage access to service-scoped secrets in `crates/moto-keybox/src/abac.rs` `evaluate_service()` — garages should NOT be able to read `ai-proxy/*` secrets directly. Add a deny-list for sensitive service prefixes or require explicit grant.
- **garage-isolation v0.5 (2026-03-11):** Add IPv6 NetworkPolicy rules in `crates/moto-club-k8s/src/network_policy.rs` — mirror all IPv4 egress rules with IPv6 equivalents. Block `fd00::/8` (ULA/WireGuard overlay), `::1/128` (loopback), `fe80::/10` (link-local).
- **ai-proxy v0.5 (2026-03-11):** Verify Ed25519 SVID signature in `crates/moto-ai-proxy/src/auth.rs` `extract_garage_id()` — load keybox's public verifying key at startup, verify signature before trusting claims. A forged JWT MUST be rejected.
- **keybox v0.16 (2026-03-11):** Add service token authentication to `POST /auth/token` in `crates/moto-keybox/src/api.rs` and `pg_api.rs` — only moto-club (via service token) should be able to issue SVIDs. Return 401 for unauthenticated callers.
- **moto-cli v0.14 (2026-03-10):** Fix `garage extend --ttl` default from `2h` to `4h` to match spec
- **service-deploy v0.7 (2026-03-10):** Update spec replica counts (1→3) and resource values to match bike.toml
- **local-dev v0.11 (2026-03-10):** Add moto-ai-proxy to spec's `moto dev up` flow (port assignments, env vars, step 9/10, --no-ai-proxy flag)
- **dev-container v0.19 (2026-03-10):** Fix SVID mount path in spec from `/run/svid` to `/var/run/secrets/svid`
- **garage-isolation v0.5 (2026-03-10):** Fix endpoint name in spec from `POST /auth/issue-svid` to `POST /auth/issue-garage-svid`

## moto-club v2.3

- moto-club-types crate: GarageId, GarageState, GarageInfo
- moto-club-wg crate: lib.rs, ipam.rs, peers.rs, sessions.rs, derp.rs (in-memory)
- moto-club-db crate: lib.rs, models.rs, garage_repo.rs (scaffolding)
- moto-club-api crate: lib.rs, health.rs, garages.rs, wg.rs (scaffolding)
- moto-club-k8s crate: lib.rs, namespace.rs, pods.rs (scaffolding)
- moto-club-garage crate: lib.rs, service.rs, lifecycle.rs (scaffolding)
- moto-club-reconcile crate: lib.rs, garage.rs (scaffolding)
- moto-club binary: main.rs (scaffolding)
- Device identity model: WireGuard public_key as primary key (spec lines 406, 1040-1046)
- moto-club-db: PostgreSQL migrations for all tables (garages, wg_devices, wg_sessions, wg_garages)
- moto-club-db: models updated for spec v1.1 (removed Attached status, WgDevice uses public_key as PK, added WgGarage model, Garage has image field)
- moto-club-db: wg_devices repository using public_key as primary key (wg_device_repo.rs)
- moto-club-db: wg_sessions repository with garage_id ON DELETE CASCADE (wg_session_repo.rs)
- moto-club-db: wg_garages repository with deterministic IP allocation (wg_garage_repo.rs)
- moto-club-api: PostgreSQL storage implementations (postgres_stores.rs: PostgresPeerStore, PostgresSessionStore)
- moto-club-api: GET /api/v1/wg/garages/{garage_id} endpoint (returns registration info for garage pods)
- moto-club-api: K8s ServiceAccount token validation for garage endpoints (moto-k8s TokenReviewOps trait, validate_garage_token helper)
- moto-club-api: GET /api/v1/wg/derp-map endpoint (returns DERP map with version for clients and garages)
- moto-club-api: Conditional GET for peers (?version= param, 304 response)
- moto-k8s: Labels use moto.dev/garage-id and moto.dev/garage-name per spec (labels.rs updated, all usages fixed)
- moto-club: Structured JSON logging (main.rs: flatten_event=true for flat JSON output per spec lines 1183-1194)
- moto-club-api: K8s namespace deletion in close flow (DELETE /api/v1/garages/{name} calls GarageK8s.delete_garage_namespace per spec lines 903-913)
- moto-club-api: GET /api/v1/info includes api_version, git_sha, features fields per spec lines 803-817
- moto-club-api: POST /api/v1/garages uses GarageService for full K8s integration
- moto-club-api: Removed unused SESSION_EXPIRED error code (spec v1.0 changelog)
- moto-club-api: GET /api/v1/wg/sessions endpoint with ?garage_id and ?all query params per spec lines 514-540
- moto-club-api: GET /health endpoint includes database, k8s, and keybox checks per spec lines 1153-1179
- moto-club-api: GET /api/v1/garages query params ?status= and ?all= per spec lines 295-300 (with INVALID_STATUS error code)
- moto-club-api: POST /api/v1/garages/{name}/extend returns ExtendTtlResponse {expires_at, ttl_remaining_seconds} per spec lines 379-386
- Remove SSH key management (v1.2 changelog: ttyd+WireGuard tunnel is sole auth boundary): moto-club-wg/src/ssh_keys.rs, moto-club-db user_ssh_key_repo.rs and user_ssh_keys table, UserSshKey model, SSH key API endpoints, PostgresSshKeyStore, moto-club-k8s secrets.rs (SshKeysSecretOps) and SSH volume mount in pods.rs, SSH key Secret step in garage service, INVALID_SSH_KEY/SSH_KEY_NOT_FOUND/SSH_KEY_NOT_OWNED error codes, ssh_key_manager in AppState
- Clean up outdated SSH comments in service.rs and garages.rs
- Create workspace PVC in garage create flow (spec v1.3 step 10: service.rs calls GarageWorkspacePvcOps.create_workspace_pvc before deploying pod)
- WireGuard keypair generation in garage create flow (spec v1.3 step 7: create wireguard-config ConfigMap and wireguard-keys Secret; GarageWireGuardOps trait, WireGuardResources struct, service.rs integration)
- Issue garage SVID from keybox in garage create flow (spec v1.3 step 8: moto-club-garage KeyboxClient, moto-club-k8s GarageSvidOps trait, service.rs integration with optional KeyboxClient)
- Fix: GET /api/v1/info features.websocket returns true (v1.4: WS /internal/wg/garages/{id}/peers implemented)
- Fix: Call create_garage_postgres() and create_garage_redis() in garage creation flow (v1.4: service.rs now calls GaragePostgresOps.create_garage_postgres and GarageRedisOps.create_garage_redis when with_postgres/with_redis are true)
- /health/ready and /health keybox integration (v1.5: checks keybox /health/ready on port 8081, returns degraded status if unreachable, adds keybox field to response; MOTO_CLUB_KEYBOX_URL env var for config; AppState.keybox_url field)
- Store garage public_key in wg_garages table during creation (v1.5: step 7 - service.rs calls wg_garage_repo::register after creating WireGuard resources, endpoints empty initially)
- Add owner field to RegisteredDevice and DeviceRegistration structs (v1.5: moto-club-wg peers.rs adds owner field, PostgresPeerStore now uses device.owner instead of hardcoded "unknown")
- Consolidate status enums (v1.6: remove GarageState and GarageInfo from moto-club-types/src/garage.rs; GarageStatus in moto-club-db/src/models.rs is now the single source of truth)
- Extract moto-club-ws crate (v1.6: WebSocket handlers moved from moto-club-api/src/wg.rs to moto-club-ws crate with PeerStreamingContext trait; AppState implements trait for peer streaming)
- Separate test files for wg.rs (v1.6: moved tests from moto-club-api/src/wg.rs to wg_test.rs per AGENTS.md test organization convention)
- Separate test files for pods.rs (v1.6: moved tests from moto-club-k8s/src/pods.rs to pods_test.rs per AGENTS.md test organization convention)
- Remove in-memory storage (v1.6: deleted InMemoryPeerStore/InMemoryStore re-exports from moto-club-api; added PostgresIpamStore; updated AppState and main.rs to use PostgreSQL storage exclusively; handler tests now require PostgreSQL)
- Simplify DERP configuration (v1.7: replace config file + database storage with MOTO_CLUB_DERP_SERVERS JSON env var; delete derp_servers table, derp_server_repo.rs, DerpServer model, DerpStore trait, DerpMapManager, InMemoryDerpStore, config file loading; add parse_derp_servers_env function; AppState uses Arc<DerpMap> instead of DerpMapManager)
- Remove InMemoryStore from moto-club-wg ipam.rs (v1.7: deleted InMemoryStore, converted tests to unit tests for pure functions only; updated lib.rs exports; added integration feature flag; updated doc examples)
- Remove InMemoryPeerStore from moto-club-wg peers.rs (v1.7: deleted InMemoryPeerStore struct and impl; removed HashMap and Mutex imports; removed export from lib.rs; existing tests are already unit tests for serialization or marked as requiring PostgreSQL)
- Remove InMemorySessionStore from moto-club-wg sessions.rs (v1.7: deleted InMemorySessionStore struct and impl; removed HashMap and Mutex imports; removed export from lib.rs; existing tests are already unit tests for Session methods and serde)
- Convert ignored integration tests to use moto-test-utils (v1.8: moto-club-api/src/wg_test.rs handler_tests module now uses `#[cfg(feature = "integration")]` instead of `#[ignore]`; tests use test_pool() for database connection and unique_owner() for test isolation)
- Add MOTO_CLUB_KEYBOX_HEALTH_URL env var (v1.9: configures keybox health check endpoint separately from API URL; defaults to MOTO_CLUB_KEYBOX_URL with port replaced by 8081; AppState.keybox_health_url field replaces keybox_url; check_keybox uses URL directly instead of hardcoded port replacement)
- Add MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE env var (v2.0: reads service token from file for keybox authentication; Config.keybox_service_token field; when both KEYBOX_URL and service token are configured, creates KeyboxClient and uses GarageService::with_keybox for SVID issuance)
- Fix moto.dev/expires-at namespace label to use unix timestamp (v2.0: namespace.rs uses dt.timestamp() instead of dt.to_rfc3339(); labels.rs doc comment updated; colons and plus signs in RFC 3339 are invalid K8s label values)
- Fix: GarageResponse includes updated_at field from database model (v2.2: added updated_at: DateTime<Utc> to GarageResponse struct and From<Garage> impl in garages.rs)
- Fix: /health/ready on port 8081 includes K8s API reachability check (v2.2: ready_handler now checks database, K8s API, and keybox; K8s failure degrades but doesn't fail; uses existing check_k8s function with state.k8s_client)
- Embed migrations and auto-run on startup (v2.3: moto-club-db adds MIGRATIONS static with sqlx::migrate!(), run_migrations() function, Migration error variant; moto-club main.rs calls run_migrations() after connect() before serving requests; same pattern as moto-keybox-db)
- ClusterRole for K8s operations (v2.3: defined in infra/k8s/moto-system/club.yaml via service-deploy.md; ClusterRole with 11 resource types including namespaces with patch, ClusterRoleBinding to moto-club ServiceAccount)

## moto-club v2.4

(spec-only update — fixes pod_name in examples, /health response format, /health/ready behavior, documents version field in derp-map response)

## moto-club v2.5

(spec-only update — namespace format documented as short_id)

## moto-club bug-fix

- POST /api/v1/wg/devices always returns 201: fixed to return 200 for idempotent re-registration of existing device, 201 only for new registrations (PeerRegistry::register_device now returns (RegisteredDevice, bool) tuple; added register_device_reregistration_returns_200 test)
- DeviceResponse missing `created_at` field: added `created_at: DateTime<Utc>` to `RegisteredDevice` (moto-club-wg/peers.rs), `DeviceResponse` (moto-club-api/wg.rs), postgres store mapping, and CLI client's `DeviceResponse` (moto-cli-wgtunnel/client.rs)
- `DEVICE_NOT_OWNED` and `SESSION_NOT_OWNED` error codes not defined: added both constants to `error_codes` module in `moto-club-api/src/lib.rs` — spec requires them for ownership checks (403)
- TTL env vars not read from environment: replaced hardcoded TTL constants in `moto-club-garage/src/lib.rs` with `LazyLock` statics reading `MOTO_CLUB_MIN_TTL_SECONDS`, `MOTO_CLUB_DEFAULT_TTL_SECONDS`, `MOTO_CLUB_MAX_TTL_SECONDS` env vars (with same defaults); removed duplicate constants in `moto-club-api/src/garages.rs` in favor of imports
- Namespace naming mismatch breaks token validation: `wg.rs:384` used full UUID in `format!("moto-garage-{garage_id}")` but `service.rs` and `namespace.rs` use `garage_id.short()` (8-char prefix) — fixed to use `&garage_id[..8]` for consistent namespace matching
- GARAGE_NOT_REGISTERED swallowed as INTERNAL_ERROR: added `GARAGE_NOT_REGISTERED` error code to `error_codes` module; updated `create_session` in `wg.rs` to match on `SessionError::GarageNotRegistered` and return 400 with `GARAGE_NOT_REGISTERED` instead of mapping all errors to `INTERNAL_ERROR`
- Session creation missing ownership/expiry/termination checks: added `validate_garage_for_session` helper that looks up garage from DB and checks ownership (`GARAGE_NOT_OWNED` 403), termination (`GARAGE_TERMINATED` 410), and expiry (`GARAGE_EXPIRED` 410); also added `DEVICE_NOT_OWNED` check; updated integration tests to use owner-aware garage creation
- `close_session` missing ownership check: handler now calls `wg_session_repo::verify_ownership` before closing, returns 403 `SESSION_NOT_OWNED` for sessions belonging to a different user (ownership determined via device FK); added `close_session_not_owned` integration test
- `get_device` missing ownership check: handler extracted owner with `let _owner = ...` but never compared it to the device's owner. Fixed to return 403 `DEVICE_NOT_OWNED` when device belongs to a different user.
- Fallback TTL validation ignores `MIN_TTL_SECONDS`: `garages.rs` fallback path checked `ttl_seconds <= 0` instead of `< *MIN_TTL_SECONDS`, accepting values 1-299 that should be rejected (minimum is 300s). Fixed to use `MIN_TTL_SECONDS` consistent with the `GarageService` path.
- Fallback `create_garage` writes full UUID namespace: `garages.rs` used `format!("moto-garage-{id}")` with full UUID, but `service.rs` and `namespace.rs` use `garage_id.short()` (8-char prefix). Fixed to use `GarageId::from_uuid(id).short()` for consistent namespace naming.
- Fallback `create_garage` collision-retry for auto-generated names: name collision returned 409 instead of transparent retry. Fixed to retry up to 3 times with random 4-char alphanumeric suffix (e.g., "bold-mongoose-7x2k"), returning `INTERNAL_ERROR` on exhaustion. User-provided names still return 409.
- `MOTO_CLUB_DEV_CONTAINER_IMAGE` env var not fully wired: `main.rs` reads the env var and passes it to `GarageK8s`, but `DEFAULT_IMAGE` in `moto-club-garage/src/lib.rs` and the fallback path in `garages.rs` hardcoded `"ghcr.io/nhalm/moto-dev:latest"`. Fixed: `DEFAULT_IMAGE` now reads from env var via `LazyLock`, `service.rs` uses `GarageK8s.dev_container_image()`, API fallback queries `GarageK8s` or falls back to `DEFAULT_IMAGE`. Also fixed `GarageK8s::new()` default to match spec.
- GET `/api/v1/wg/garages/{id}` returns dummy `peer_version` and `registered_at`: handler hardcoded `peer_version: 0` and `registered_at: Utc::now()` instead of using database values. Fixed by adding `peer_version` and `registered_at` fields to `RegisteredGarage` struct, populating from `WgGarage` in PostgreSQL store, and using `garage.peer_version`/`garage.registered_at` in handler.
- `register_garage` returns FK error instead of `GARAGE_NOT_FOUND` 404: added garage existence check via `garage_repo::get_by_id` before upserting into `wg_garages`, returning proper 404 `GARAGE_NOT_FOUND` instead of FK constraint violation surfacing as generic internal error
- Session TTL not capped to garage's remaining TTL: added `garage_expires_at` field to `CreateSessionRequest` in `sessions.rs`, capped `expires_at` via `.min(request.garage_expires_at)` in `SessionManager::create_session`; updated `validate_garage_for_session` in `wg.rs` to return garage's `expires_at` so the API handler can pass it through
- `delete_garage` does not close active sessions or broadcast peer removals: added `session_manager.on_garage_terminated()` call after DB termination to close all active sessions and increment `peer_version`, then `peer_broadcaster.broadcast_remove()` for each closed session to notify connected garages.

---

## moto-wgtunnel v0.10

- Implement daemon `run()` event loop: register with moto-club, spawn health HTTP server (axum), init WireGuard tunnel engine (per-peer Tunnel map), main tokio::select! loop with SIGTERM + timer ticks + shutdown channel, graceful cleanup

## moto-wgtunnel v0.9

- moto-wgtunnel-types crate: keys.rs, ip.rs, peer.rs, derp.rs
- moto-wgtunnel-derp crate: protocol.rs, client.rs, map.rs
- moto-wgtunnel-conn crate: stun.rs, endpoint.rs, path.rs, magic.rs
- moto-wgtunnel-engine crate: config.rs, tunnel.rs, platform/
- moto-cli-wgtunnel crate: tunnel.rs, status.rs, enter.rs, ttyd.rs (complete - v0.9 updated status)
- moto-garage-wgtunnel crate: register.rs, health.rs, daemon.rs
- enter.rs: MagicConn for direct UDP
- enter.rs: DerpClient for DERP relay fallback
- enter.rs: ttyd WebSocket terminal connection (replaces SSH per spec v0.8)
- client.rs: Device registration via moto-club API (POST /api/v1/wg/devices using WG public key as device identity per spec v0.7)
- client.rs: Session creation via moto-club API (GET garage by name, POST session with garage UUID and device pubkey per spec)
- client.rs: Get garage details for session creation (GET /api/v1/garages/{name} returns garage UUID needed for session)
- tunnel.rs: Remove device_id from DeviceIdentity (per spec v0.7: WG public key IS device identity)
- Remove SSH key management from moto-garage-wgtunnel (spec v0.8: ttyd+WireGuard tunnel is sole auth boundary)
- Remove dead SSH code from moto-cli-wgtunnel/src/enter.rs (SshConfig, spawn_ssh, etc.)

---

## container-system v1.3

- (see tracks-history.md)
- Create `infra/pkgs/moto-keybox.nix` (bike base + moto-keybox-server binary, using mkBike helper)
- Export `moto-keybox-image` from flake.nix (default.nix and flake.nix updated)
- Fix `infra/pkgs/moto-club.nix` cargoHash placeholder (replaced with real hash; also fixed moto-keybox.nix; committed Cargo.lock to git for Nix flake source access)
- Switch engine builds to crane (v1.2: add crane flake input; craneLib, commonArgs, cargoArtifacts in flake.nix; moto-club.nix and moto-keybox.nix use craneLib.buildPackage; removed cargoHash from engine packages; deps built once via buildDepsOnly and shared)
- Bump Rust toolchain from 1.85 to 1.88 (v1.3: `home` crate v0.5.12 requires Rust 1.88; flake.nix rustToolchain updated to `pkgs.rust-bin.stable."1.88.0".minimal`)
- Add `stdenv.cc` and `lld` to `commonArgs.nativeBuildInputs` (v1.3: crane needs a C compiler/linker; `.cargo/config.toml` specifies `-fuse-ld=lld` for Linux targets)

## container-system.md bug-fix

- Makefile: fix registry-start port from 5000 to 5050

---

## container-system.md bug-fix (2)

- Add `[profile.release]` section to root `Cargo.toml` with `lto = true`, `codegen-units = 1`, `strip = true` per spec.
- Update `Cargo.toml` `rust-version` from `"1.85"` to `"1.88"` to match `flake.nix` toolchain pin.

---

## moto-cli v0.11

- Global flags: --json/-j, --verbose/-v (counted), --quiet/-q, --context/-c, --help/-h, --version/-V
- ColorMode: auto/always/never with MOTO_NO_COLOR env var support
- Configuration: XDG config path, TOML parsing, precedence (CLI > env > config > defaults)
- moto garage open: --owner, --ttl (duration parsing, min/max validation), --engine, name auto-generation
- moto garage enter: WireGuard tunnel via moto-cli-wgtunnel, SSH session spawning
- moto garage logs: --follow/-f, --tail/-n, --since (duration parsing)
- moto garage list: --context (supports "all" for multi-context), table output with context column
- moto garage close: --force, confirmation prompt
- moto bike build: --tag (default: git sha), --push (MOTO_REGISTRY env var), Docker-wrapped Nix
- moto bike deploy: --image, --replicas, --wait, --wait-timeout, --namespace/-n
- moto bike list: --namespace/-n, table output
- moto bike logs: --follow/-f, --tail/-n, --since, --namespace
- moto cluster init: --force, k3d cluster creation, idempotent, registry setup
- moto cluster status: API health check, registry health check, JSON output
- Exit codes: 0 (success), 1 (general), 2 (not found), 3 (invalid input)
- Actionable error messages with suggestions
- --branch flag on garage open (tracked in garage-lifecycle.md v0.4)
- --no-attach flag on garage open (tracked in garage-lifecycle.md v0.4)
- Fix: --owner flag passed to API (v0.4)
- Fix: Implement garage logs command (v0.4)
- Fix: `cluster init --json` output matches spec (v0.5: added `type` field with value "k3d", removed non-spec `api_endpoint`/`registry_endpoint` fields; JSON now emits `name`, `type`, `status` per spec)
- Fix: `garage logs` respects `--context` global flag when creating K8s client (v0.5: uses K8sClient::with_context when --context flag is set, otherwise falls back to default context)
- Fix: `garage list --context <name>` filters results by context (v0.5: garages from the current moto-club belong to the current kubectl context; when --context targets a different context, no garages are shown since that context's moto-club is not queried)
- `moto dev` subcommand: `dev status` health check dashboard (v0.6: dev subcommand in command hierarchy with up/down/status; status checks cluster, registry, postgres, keybox, club, image, garages; JSON output; exit code 0/1)
- `moto dev down` command implementation (v0.6: SIGTERM to port processes via lsof, docker compose down, --clean flag removes .dev/ and pgdata volume; DevConfig.keybox_api field for port lookup)
- `moto dev up` command implementation (v0.6: 9-step orchestration with --no-garage/--rebuild-image/--skip-image flags; DevConfig env var methods for subprocess spawning; prerequisites/cluster/image/postgres/keys/migrations/keybox/club/garage steps; subprocess management with tokio::process; health check with exponential backoff; Ctrl-C handling kills subprocesses; JSON output; idempotent restart)
- `--kubectl` flag on `garage enter` and `garage open` (v0.8: connects via `kubectl exec -it -n {namespace} {pod_name} -- tmux attach-session -t garage` instead of WireGuard tunnel; namespace/pod_name from API response with fallback to `moto-garage-{id[..8]}`/`dev-container`; respects --context flag)
- Config file `user` field and MOTO_USER env var for user identity (v0.9: Config.user top-level field in config.toml; owner precedence: --owner flag > MOTO_USER env var > config file user > error with actionable message)
- Fix: Config path uses `$HOME/.config/moto/config.toml` directly instead of `dirs::config_dir()` (v0.10: avoids macOS `~/Library/Application Support/` path; respects `$XDG_CONFIG_HOME` if set; removed `dirs` dependency from moto-cli)
- Fix: `--kubectl` uses `tmux new-session -A -s garage` instead of `tmux attach-session -t garage` (v0.11: `-A` creates the session if it doesn't exist, matching ttyd behavior)

---

## dev-container v0.17

- Nix dockerTools.buildLayeredImage with buildEnv wrapper
- Modular structure: infra/pkgs/moto-garage.nix, infra/modules/{base,dev-tools,terminal,wireguard}.nix
- Root flake at moto/flake.nix exports moto-garage package
- Multi-arch via eachDefaultSystem (x86_64-linux, aarch64-linux)
- Rust 1.85 stable toolchain with extensions (rust-src, rust-analyzer, rustfmt, clippy)
- Rust tools: cargo-watch, cargo-nextest, mold, sccache, sqlx-cli
- System libraries: pkg-config, openssl, postgresql.lib
- Version control: git, jujutsu, gh
- Database clients: postgresql
- General tools: curl, jq, yq, ripgrep, fd, bat, htop, tree
- Kubernetes: kubectl
- Node.js 22.x LTS
- Connectivity: wireguard-tools, ttyd, tmux (no openssh - WireGuard tunnel is auth boundary)
- Environment variables: WORKSPACE, CARGO_HOME, CARGO_TARGET_DIR, RUST_BACKTRACE, RUST_LOG, RUSTC_WRAPPER, RUSTFLAGS, NIX_PATH, SSL_CERT_FILE, DO_NOT_TRACK
- Container config: garage-entrypoint cmd (starts ttyd), /workspace workdir, volumes, port 7681 exposed
- Terminal daemon: ttyd on port 7681 with tmux session persistence (terminal.nix module)
- Smoke tests: infra/smoke-test.sh (core tools, terminal tools, env vars, Rust compilation)
- v0.14 clarifications: Claude Code installed at runtime (not build time), Cmd is garage-entrypoint, K8s env vars injected by K8s (already implemented correctly)
- Reduce image size: remove cargo-audit, cargo-deny, cargo-edit, cargo-expand from container (v0.15: CI tools not needed in dev container)
- Reduce image size: remove k9s and helm from container (v0.15: kubectl is sufficient)
- Reduce image size: remove redis package from container (v0.15: redis-cli available via supporting service container)
- Reduce image size: switch Rust toolchain from .default to .minimal profile, add rustfmt+clippy extensions explicitly (v0.16: excludes rust-docs, ~700MB savings)
- Reduce image size: drop clang from container, update RUSTFLAGS to `-C link-arg=-fuse-ld=mold` (v0.16: ~1.4GB savings, use default cc linker with mold)
- Remove /nix volume declaration from container image config (v0.16: Docker VOLUME for /nix shadows image's /nix/store contents)

---

## local-cluster v0.3

- moto cluster init: k3d cluster creation with moto name
- k3d create args: --api-port 6550, --port 80:80, --port 443:443, --registry-create moto-registry:0.0.0.0:5050, --disable=traefik
- Idempotent: returns success if cluster already exists (unless --force)
- Docker running check
- Wait for API ready
- moto cluster status: cluster info, API health, registry health
- JSON output format with name, type, status, api, registry
- Status values: running, stopped, not_found
- Exit codes: 0 running, 1 not running/error
- --force flag to delete and recreate
- moto cluster init JSON output: status "created" or "exists" (v0.2 changelog: ClusterInitJson struct with name, status, api_endpoint, registry_endpoint; --json flag produces "created" for new clusters, "exists" for idempotent case)
- Change registry port from 5000 to 5050 (v0.3: avoids macOS AirPlay Receiver conflict; --registry-create moto-registry:0.0.0.0:5050 format binds to all interfaces)

---

## makefile v0.15

- Setup targets: install (git hooks)
- Development targets: build, test, check, fmt, lint, clean, run, fix, ci
- Container targets: build-garage, test-garage, shell-garage, push-garage, scan-garage, clean-images, clean-nix-cache
- Bike targets: build-bike, test-bike
- Registry targets: registry-start, registry-stop
- Docker-wrapped Nix build (NIX_LINUX_SYSTEM auto-detection)
- nix-store volume for caching
- REGISTRY env var support (default: localhost:5000)
- SHA tagging from git
- .PHONY declarations for all targets
- Testing targets: test-db-up, test-db-down, test-db-migrate, test-integration, test-all, test-ci
- Service container targets: build-club, push-club, build-keybox, push-keybox
- Local dev targets: dev-db-up, dev-db-down, dev-db-migrate, dev-keybox-init, dev-keybox, dev-club, dev-garage-image, dev-up, dev-down, dev-clean
- Local dev target: dev-cluster (k3d cluster creation via moto CLI)
- Deploy targets: deploy-secrets, deploy-system, deploy-status, undeploy-system (v0.9: implemented via service-deploy.md; idempotent credential generation, kubectl apply -k, rollout wait with status, namespace+RBAC cleanup)
- deploy-images target: builds and pushes all three service images (garage, club, keybox) to local registry (v0.10)
- `deploy` target: full deployment flow (deploy-images + deploy-secrets + deploy-system + deploy-status) (v0.10)
- push-club and push-keybox clean up local Docker images after pushing (v0.11: same as push-garage; saves disk space since images only need to live in the registry)
- `help` as default target: `.DEFAULT_GOAL := help`; `make` with no arguments prints all available targets grouped by category (v0.12: inline `##` comments on targets, `##@` section headers, awk-based help parser)
- `test-all` runs every test category: unit + integration + ignored (K8s); each category runs exactly once (v0.13: single `cargo test --features integration` pass for unit+integration, separate `cargo test -- --ignored` for K8s tests; no duplicate unit test runs)
- `dev-cluster-down` target: deletes k3d cluster and local registry via `k3d cluster delete moto` (v0.14)
- `make install` builds release binary and copies to `~/.local/bin/moto` (v0.15: `cargo build --release --bin moto` + `cp target/release/moto ~/.local/bin/moto`)

## makefile v0.16

- Fix `test` target description: "Run unit tests" (was misleadingly "Run all tests")
- Add `build-bike` and `test-bike` to Container Targets spec section (spec-only — already in Makefile per v0.6)
- Remove stale `localhost:5000` from `push-garage` comment (registry port is determined by REGISTRY variable)

## makefile v0.17

(spec-only update — no code changes needed, targets already exist)

---

## moto-bike v0.3

- Bike base image (infra/pkgs/moto-bike.nix): CA certs, tzdata, non-root user (1000:1000), security context
- mkBike helper function for building final images from bike base + engine binary
- Flake exports moto-bike package and mkBike lib helper
- bike.toml for moto-club engine (crates/moto-club/bike.toml)
- Engine health endpoints: /health/live, /health/ready, /health/startup on port 8081 (moto-club-api health.rs, moto-club main.rs)
- Final bike images in flake: moto-club-image using mkBike helper (infra/pkgs/moto-club.nix, flake.nix exports packages.{x86_64,aarch64}-linux.moto-club-image)
- Engine Contract: Prometheus metrics endpoint on port 9090 (moto-club main.rs with metrics-exporter-prometheus, moto-club-api metrics.rs with http_requests_total and http_request_duration_seconds, process metrics via metrics-process)
- Engine Contract: Graceful shutdown (SIGTERM handling, 30s grace period) - moto-club main.rs shutdown_signal() with tokio::signal

---

## moto-bike bug-fix

- bike.toml: update replicas from 2 to 3 (crates/moto-club/bike.toml deploy.replicas, crates/moto-cli/src/bike.rs default_replicas)
- K8s manifest: add RUST_LOG="info" env var (crates/moto-k8s/src/deployment.rs build_deployment container env)
- K8s manifest: add POD_NAME and POD_NAMESPACE via downward API (crates/moto-k8s/src/deployment.rs build_deployment container env)
- K8s manifest: add rolling update strategy (maxSurge: 1, maxUnavailable: 0)
- K8s manifest: add container-level securityContext (readOnlyRootFilesystem, allowPrivilegeEscalation, capabilities)
- Create `crates/moto-keybox-server/bike.toml` (name="keybox", replicas=3, resources per spec)
- Keybox manifest: add security baseline (POD_NAME, POD_NAMESPACE, RUST_LOG, rolling update strategy, pod securityContext, container securityContext) to infra/k8s/moto-system/keybox.yaml

---

## garage-lifecycle v0.4

- moto garage extend CLI command: --ttl flag (default 2h), duration parsing, max TTL validation
- moto-garage: GarageClient.extend() method updates namespace labels with new expires_at
- moto-k8s: NamespaceOps.patch_namespace_labels() for updating namespace labels via merge patch
- JSON output for extend command (name, expires_at, ttl_remaining_seconds)
- Dev container: ttyd daemon on port 7681 with tmux for session persistence (garage-entrypoint script, container Cmd updated)
- moto garage enter: ttyd WebSocket client (moto-cli-wgtunnel ttyd.rs), replaces SSH with WebSocket to port 7681
- Ready criteria check: WireGuard registration check in reconciler (garage transitions to Ready only when wg_garages entry exists)
- Ready criteria check: ttyd accepting connections (K8s TCP readiness probe on port 7681 in garage pod spec)
- Repo cloning: init container with REPO_URL, REPO_BRANCH, REPO_NAME env vars (moto-club-k8s pods.rs RepoConfig, build_repo_clone_init_container); workspace volume shared between init and main container; 3-retry clone logic
- 5-state lifecycle: Rename Running to Initializing, add Failed state per spec v0.3 changelog (GarageStatus enum, GarageState enum, lifecycle state machine, reconciler mapping, API status parsing)
- CLI --branch flag for garage open (v0.4: passes branch to CreateGarageRequest)
- CLI --no-attach flag for garage open (v0.4: creates garage without connecting; default is to connect after creation)
- Ready criteria check: repo cloned (v0.4: reconciler checks init container completed successfully via init_container_succeeded method in GaragePodOps trait, moto-club-k8s pods.rs)
- Fix garage open output format to match spec (v0.4: show ID, branch, expires_at, status) - moto-cli/src/commands/garage.rs, GarageOpenJson struct updated, format_short_id and format_expires_at helpers added
- Fix garage list columns to match spec (v0.4: add ID, BRANCH columns) - moto-cli/src/commands/garage.rs, GarageJson struct updated, table header and rows now show ID, NAME, BRANCH, STATUS, TTL, AGE columns

---

## keybox v0.11

- moto-keybox library: SPIFFE ID types (garage/bike/service), SVID claims, SvidIssuer, SvidValidator
- moto-keybox: Envelope encryption (MasterKey, DataEncryptionKey, EncryptedDek, EncryptedSecret)
- moto-keybox: ABAC PolicyEngine with hardcoded policies per spec (MVP)
- moto-keybox: SecretRepository (in-memory) with CRUD operations per scope
- moto-keybox: REST API handlers (POST /auth/token, GET/POST/DELETE /secrets/{scope}/{name}, GET /secrets/{scope}, GET /audit/logs)
- moto-keybox-db: models (Secret, SecretVersion, EncryptedDek, AuditLogEntry)
- moto-keybox-db: PostgreSQL migrations (initial schema)
- moto-keybox-db: connect, run_migrations, MIGRATIONS embedded
- moto-keybox-client: KeyboxClient with K8s mode and local mode support
- moto-keybox-client: SvidCache with automatic refresh
- moto-keybox-cli: init command (generates KEK and SVID signing key)
- moto-keybox-cli: issue-dev-svid command (24h dev SVID for local testing)
- moto-keybox-cli: set/get/list secret commands
- moto-keybox-server: Server binary (main.rs) with config from env vars, graceful shutdown, JSON logging
- POST /auth/issue-garage-svid endpoint for moto-club delegation (per spec v0.3 changelog: garage SVID issuance with 1-hour TTL, service token auth, IssueGarageSvidRequest/Response types)
- Service token authentication for moto-club (MOTO_KEYBOX_SERVICE_TOKEN and MOTO_KEYBOX_SERVICE_TOKEN_FILE env vars, constant-time comparison)
- 1 MB maximum secret size limit in API validation (v0.4: MAX_SECRET_SIZE_BYTES constant, validation in set_secret handler, SECRET_TOO_LARGE error code)
- Return 403 Forbidden for both "not found" and "access denied" to prevent secret enumeration (v0.4: map_error returns ACCESS_DENIED for both SecretNotFound and AccessDenied errors, updated client to remove dead SECRET_NOT_FOUND code path)
- Health check endpoints per moto-bike.md spec (v0.4: /health/live, /health/ready, /health/startup on port 8081 via moto-keybox-server, health.rs module in moto-keybox)
- Wire up moto-keybox-db PostgreSQL backend for secrets and audit logs (v0.4: secret_repo.rs, audit_repo.rs in moto-keybox-db; PgSecretRepository in moto-keybox/pg_repository.rs; PgAppState and pg_router in moto-keybox/pg_api.rs; server uses MOTO_KEYBOX_DATABASE_URL env var to enable PostgreSQL mode)
- Fix bikes ABAC: enforce service field matching (v0.4: SvidClaims.service field added; bikes must have service claim to access service-scoped secrets; ABAC evaluate_service checks principal.service == resource.service)
- Rename moto-keybox-server binary from moto-keybox to moto-keybox-server (v0.5: fixes cargo doc collision with moto-keybox library crate)
- v0.6 integration tests: no ignored tests exist in moto-keybox or moto-keybox-db to convert (all existing tests are unit tests that don't require PostgreSQL)
- Fix: Secret retrieval handlers enforce pod UID binding (v0.8: get_secret, set_secret, delete_secret now call validate_enforcing_pod_uid() instead of validate(); SvidValidator.validate_enforcing_pod_uid validates pod_uid claim is non-empty when present; both api.rs and pg_api.rs updated with extract_svid_enforcing_pod_uid helper)
- Enforce endpoint authorization matrix (v0.10: set_secret and delete_secret require service token only, deny SVID with 403 FORBIDDEN; get_secret and list_secrets accept both service token (skip ABAC) and SVID (ABAC checked); get_audit_logs requires service token only; both api.rs and pg_api.rs updated; AppState/PgAppState store admin_service for synthetic claims; FORBIDDEN error code used for wrong token type)
- POST /admin/rotate-dek/{name} endpoint (v0.10: rotates DEK for a secret, re-encrypts value with new DEK, creates new version; service token only; ?scope= query param for scope/service/instance; DekRotated audit event type added to both moto-keybox types.rs and moto-keybox-db models.rs; rotate_dek method on both SecretRepository and PgSecretRepository; handler in both api.rs and pg_api.rs; route registered in both routers; 404 SECRET_NOT_FOUND if secret doesn't exist)
- Auth matrix enforcement tests (v0.11: 8 handler-level tests in api_test.rs using in-memory router with tower::ServiceExt::oneshot; tests set_secret/delete_secret/get_audit_logs deny SVID with 403 FORBIDDEN; tests get_secret/list_secrets/get_audit_logs succeed with service token; tests get_secret/list_secrets succeed with valid SVID)
- DEK rotation tests (v0.11: 6 handler-level tests in api_test.rs; rotate_dek with SVID returns 403 FORBIDDEN; rotate_dek with service token succeeds and returns new version; rotate_dek for non-existent secret returns 404 SECRET_NOT_FOUND; secret value readable after rotation with plaintext unchanged; version incremented across multiple rotations; dek_rotated audit event logged)

## keybox v0.12

(spec-only update — documents K8s TokenReview as MVP-deferred, garage ABAC policies, DATABASE_URL as optional, extra list endpoints, SERVICE_TOKEN env var, ADMIN_SERVICE env var)

## keybox v0.13

- (spec-only) Fix `MOTO_KEYBOX_SERVICE_TOKEN` example value

## keybox bug-fix

- `/health/ready` checks DB connection at runtime: `health_router()` now accepts `Option<DbPool>`, `ready_handler` runs `SELECT 1` against the pool when using PostgreSQL backend; `main.rs` restructured to create pool before health router so it can be shared; returns 503 `not_ready` if DB is unreachable
- Fix ABAC service global-secret prefix check too broad: removed bare `starts_with(principal_id)` fallback in `evaluate_global`; now only checks `starts_with(principal_id + "/")` so service `ai` cannot access `ai-proxy/` secrets
- Fix `POST /auth/token` ignoring `MOTO_KEYBOX_SVID_TTL_SECONDS`: both `api.rs` and `pg_api.rs` `issue_token` handlers used hardcoded `DEFAULT_SVID_TTL_SECS` (900s) instead of `state.svid_issuer.ttl_secs()`; added `ttl_secs()` getter to `SvidIssuer`
- `POST /auth/token` cannot set `service` claim for bikes: added optional `service` field to `TokenRequest` (`api.rs`), wired `claims.with_service()` in both `api.rs` and `pg_api.rs` `issue_token` handlers, added `service` field to `PrincipalInfo` and `for_bike` constructor in `moto-keybox-client`

---

## garage-isolation v0.4

- Pod security context: runAsUser/runAsGroup: 0, allowPrivilegeEscalation: false, readOnlyRootFilesystem: true, seccompProfile: RuntimeDefault, capabilities (drop ALL, add CHOWN/DAC_OVERRIDE/FOWNER/SETGID/SETUID/NET_BIND_SERVICE)
- Pod spec: automountServiceAccountToken: false, host_network/host_pid/host_ipc: false
- Pod resource limits: 3 CPU / 7Gi per spec (requests: 100m CPU / 256Mi)
- Pod volumes: writable emptyDir mounts for tmp, var-tmp, home, cargo, var-lib-apt, var-cache-apt, usr-local
- Workspace PVC: workspace volume uses PersistentVolumeClaim per spec (moto-k8s PvcOps trait, moto-club-k8s GarageWorkspacePvcOps trait, pods.rs uses PVC for /workspace mount)
- Pod volumes: wireguard-config ConfigMap mount, wireguard-keys Secret mount, garage-svid Secret mount (pods.rs volumes and volumeMounts per spec)
- NetworkPolicy: garage-isolation policy per spec (moto-k8s NetworkPolicyOps trait, moto-club-k8s GarageNetworkPolicyOps trait and build_garage_isolation_policy, integrated into GarageService.create_k8s_resources)
- ResourceQuota: garage-quota per spec (moto-k8s ResourceQuotaOps trait, moto-club-k8s GarageResourceQuotaOps trait and build_garage_quota, integrated into GarageService.create_k8s_resources)
- LimitRange: garage-limits per spec (moto-k8s LimitRangeOps trait, moto-club-k8s GarageLimitRangeOps trait and build_garage_limits, integrated into GarageService.create_k8s_resources)
- Fix: remove /nix emptyDir volume and mount (v0.4: mounting emptyDir over /nix shadows the image's pre-installed /nix/store contents, breaking all tool symlinks; image provides /nix/store read-only via readOnlyRootFilesystem)

## garage-isolation.md bug-fix

- Fix NetworkPolicy keybox egress rule pod selector from `app: keybox` to `app.kubernetes.io/component: moto-keybox` to match actual keybox pod labels.

---

## supporting-services v0.3

- CLI flags: `--with-postgres` and `--with-redis` on `moto garage open` command
- API: `with_postgres` and `with_redis` fields in `CreateGarageRequest` and `CreateGarageInput`
- K8s: PostgreSQL Deployment, Service, and credentials Secret (moto-club-k8s/supporting_services.rs: GaragePostgresOps trait, build_postgres_deployment, build_postgres_service, build_postgres_credentials_secret)
- K8s: Redis Deployment, Service, and credentials Secret (moto-club-k8s/supporting_services.rs: GarageRedisOps trait, build_redis_deployment, build_redis_service, build_redis_credentials_secret)
- Garage pod: Inject Postgres env vars (POSTGRES_HOST, POSTGRES_PORT, POSTGRES_USER, POSTGRES_PASSWORD, POSTGRES_DB, DATABASE_URL per spec lines 236-255)
- Garage pod: Inject Redis env vars (REDIS_HOST, REDIS_PORT, REDIS_PASSWORD, REDIS_URL per spec lines 258-272)
- Ready check: Wait for supporting service Deployments to be available before marking garage Ready (reconciler checks postgres_available/redis_available before transitioning to Ready)
- Fix: Call create_garage_postgres() and create_garage_redis() in garage creation flow (v0.3: service.rs now calls GaragePostgresOps.create_garage_postgres and GarageRedisOps.create_garage_redis when with_postgres/with_redis are true)

---

## project-structure v1.5

- (see tracks-history.md for prior work)
- Deprecate moto-garage crate and local mode: moto-cli garage commands now use MotoClubClient HTTP client instead of moto_garage::GarageClient, removed moto-garage dependency from moto-cli, added list_garages/create_garage/close_garage/extend_garage methods to MotoClubClient
- Remove moto-garage crate entirely (v1.4: deleted crates/moto-garage/ directory, removed moto-garage from Cargo.toml workspace dependencies)

---

## testing v0.6

- docker-compose.test.yml: PostgreSQL 16-alpine on port 5433, healthcheck, test credentials (moto_test/moto_test/moto_test)
- Makefile target: test-ci (assumes database already running, runs unit + integration tests)
- Update test target to run unit tests only (cargo test --lib)
- Add `integration` feature flag to database crates (moto-club-db, moto-club-api, moto-keybox-db, moto-keybox)
- moto-test-utils crate: test_pool(), unique_garage_name(), unique_owner(), fake_wg_pubkey()
- moto-club-db integration tests: garage_repo_test.rs (15 tests)
- moto-club-db integration tests: wg_device_repo_test.rs (13 tests)
- Makefile target: test-db-up, test-db-down, test-db-migrate, test-integration, test-all
- Fix moto-club-api integration test compilation (19 tests)
- moto-club-db integration tests: wg_session_repo_test.rs (25 tests, all 11 public functions)
- moto-club-db integration tests: wg_garage_repo_test.rs (18 tests, all 7 public functions)
- moto-keybox-db integration tests: secret_repo_test.rs (28 tests, all 13 public functions)
- moto-keybox-db integration tests: audit_repo_test.rs (12 tests, all 3 public functions)
- moto-keybox-db: add not-found error path test for `update_secret_version` (v0.5: verifies `fetch_one` returns error on nonexistent ID)
- moto-keybox-db: add not-found error path test for `delete_secret` (v0.5: silently succeeds on nonexistent ID, behavior verified)
- Keybox smoke tests (v0.6: infra/smoke-test-keybox.sh with auth matrix enforcement and DEK rotation tests against live k3d cluster; `smoke-keybox` Makefile target with port-forward setup/teardown; service token from .dev/k8s-secrets/service-token; SVID token via POST /auth/token; 10 test assertions covering all spec scenarios; cleanup deletes test secrets)

## testing bug-fix

- Remove dead `integration` feature flag from `moto-keybox/Cargo.toml`: per testing spec, API/handler crates use mocked tests not integration tests; `moto-club-api` already uses the flag (wg_test.rs:224) so it stays; `moto-keybox` had zero `#[cfg(feature = "integration")]` guards
- Remove dead `integration` feature flag and empty `mod integration_tests` stub from `moto-club-wg/ipam.rs`: zero actual test functions, real integration tests live in `moto-club-api/src/wg_test.rs`

---

## local-dev v0.10

- docker-compose.yml with dev Postgres on port 5432 (postgres:16-alpine, moto/moto creds, pgdata volume, healthcheck, init script mount)
- scripts/init-dev-db.sql (creates moto_keybox database via docker-entrypoint-initdb.d)
- .dev/ added to .gitignore
- Makefile targets: dev-db-up (docker compose up --wait), dev-db-down (docker compose down), dev-db-migrate (sqlx migrate run for moto-club-db against dev database)
- Makefile target: dev-keybox-init (generate master.key, signing.key via moto-keybox init + service-token via openssl rand in .dev/keybox/; idempotent skip if all three exist)
- Makefile target: dev-keybox (start moto-keybox-server with dev env vars: port 8090/8091, .dev/keybox/ keys, PostgreSQL on localhost:5432/moto_keybox, RUST_LOG=moto_keybox=debug)
- Makefile target: dev-club (start moto-club with dev config: MOTO_CLUB_DATABASE_URL, MOTO_CLUB_KEYBOX_URL, MOTO_CLUB_DEV_CONTAINER_IMAGE, RUST_LOG env vars per spec)
- Makefile target: dev-garage-image (build-garage + push-garage to localhost:5000)
- Makefile targets: dev-down (docker compose down), dev-clean (docker compose down -v + rm .dev/)
- Makefile target: dev-up (full stack shortcut: dev-db-up + dev-keybox-init + dev-db-migrate + dev-garage-image + keybox background + moto-club foreground; Ctrl-C stops everything)
- Makefile target: dev-cluster (k3d cluster creation via moto CLI, idempotent)
- Add MOTO_CLUB_KEYBOX_HEALTH_URL=http://localhost:8091 to dev-club and dev-up targets (v0.3: keybox health port differs from API port in local dev)
- Remove dev-garage-image from dev-up prerequisites (v0.3: dev-up no longer rebuilds garage image on every run; dev-garage-image is a one-time setup step)
- Add MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE=.dev/keybox/service-token to dev-club and dev-up targets (v0.4: needed for garage SVID issuance via keybox)
- Fix MOTO_CLUB_DEV_CONTAINER_IMAGE to use moto-registry:5000 (v0.5: in-cluster k3d registry name; pods inside k3d can't reach localhost:5000)
- Update host registry push address from localhost:5000 to localhost:5050 (v0.5: matches local-cluster.md v0.3 port change)
- push-garage cleans up local Docker daemon copy after pushing to registry (v0.6: saves ~10GB VM disk; image only needs to live in registry)
- `moto dev status` command: health check dashboard for cluster, registry, postgres, keybox, club, image, garages (v0.7/v0.8: CLI scaffolding for dev subcommand with up/down/status; DevConfig with hardcoded defaults and env var overrides; JSON output; exit code 1 if any unhealthy)
- Makefile target: `dev` as alias for `moto dev up` (v0.7)
- `moto dev down` command: SIGTERM to club (port 8080) and keybox (port 8090) processes via lsof, docker compose down, --clean flag removes .dev/ directory and pgdata volume; DevConfig.keybox_api field added for port lookup (v0.7/v0.8)
- `moto dev up` command: 9-step orchestration (prerequisites, cluster, image, postgres, keys, migrations, keybox, club, garage) with subprocess management via tokio::process, health checks with exponential backoff, Ctrl-C handling, --no-garage/--rebuild-image/--skip-image flags, DevConfig env var methods, JSON output, idempotent restart (v0.7/v0.8)
- Add MOTO_CLUB_BIND_ADDR=0.0.0.0:18080 to dev-club and dev-up Makefile targets (v0.10: moto-club API port changed from 8080 to 18080 to match k3d deploy path; CLI default works for both local dev and k3d deploy modes)

## local-dev bug-fix

- Fix chmod 600 to apply to all three key files (master.key, signing.key, service-token) in both Makefile dev-keybox-init target and moto dev up ensure_keybox_keys() — previously only service-token was chmod'd

---

## service-deploy v0.5

- moto-club-db embedded migrations and auto-run on startup (prerequisite for K8s deployment — see moto-club.md v2.3)
- infra/k8s/moto-system/namespace.yaml (namespace with labels: app.kubernetes.io/part-of, app.kubernetes.io/managed-by, moto.dev/type=system)
- infra/k8s/moto-system/postgres.yaml (StatefulSet + Service on 5432 + 1Gi PVC with local-path StorageClass + postgres-init ConfigMap with CREATE DATABASE moto_keybox)
- infra/k8s/moto-system/keybox.yaml (Deployment with moto-registry:5000/moto-keybox image, resource limits 50m/128Mi→500m/512Mi, health probes on 8081, keybox-keys Secret volume at /run/secrets/keybox/ + Service on 8080+8081)
- infra/k8s/moto-system/club.yaml (Deployment with moto-registry:5000/moto-club image, resource limits 50m/128Mi→500m/512Mi, health probes on 8081, DERP_SERVERS=[], keybox-service-token Secret volume at /run/secrets/club/ + Service on 8080+8081+9090 + ServiceAccount moto-club + ClusterRole with 11 resource types including namespaces with patch + ClusterRoleBinding)
- infra/k8s/moto-system/kustomization.yaml (combines namespace, postgres, keybox, club resources)
- Makefile target: deploy-secrets (idempotent credential generation to .dev/k8s-secrets/ via moto-keybox init + openssl rand; creates namespace if needed; applies 5 K8s secrets: postgres-credentials, keybox-keys, keybox-db-credentials, club-db-credentials, keybox-service-token)
- Makefile target: deploy-system (kubectl apply -k infra/k8s/moto-system/)
- Makefile target: deploy-status (wait for rollout, show status, exit 0/1)
- Makefile target: deploy-images (v0.4: builds and pushes all three service images to local registry)
- Makefile target: `deploy` (v0.4: full deployment flow: deploy-images + deploy-secrets + deploy-system + deploy-status)
- Auto port-forward on deploy, CLI uses port 18080 (v0.5: deploy-system starts background `kubectl port-forward` from localhost:18080 to svc/moto-club:8080; CLI defaults to http://localhost:18080 via MOTO_CLUB_URL; port 18080 avoids conflicts with 80/443/8080)
- Drop `undeploy-system` target (v0.5: use `dev-cluster-down` instead; cluster deletion cleans up everything including port-forward)

## service-deploy v0.6

- (spec-only) Remove stale manual port-forward from Quick path
- (spec-only) Fix binary name: `moto-keybox init` (was `moto-keybox-cli init`)

---

## pre-commit v0.2

- .githooks/pre-commit: blocks secrets (.pem, .key, .env files)
- .githooks/pre-commit: cargo fmt --all --check (when Rust files changed)
- .githooks/pre-commit: cargo clippy --all-targets -- -D warnings (when Rust files changed, v0.2 changelog)
- .githooks/pre-commit: nix flake check --no-build (when Nix files changed)
- make install: sets git core.hooksPath to .githooks

---

## makefile.md bug-fix

- Fix `dev-down` Makefile help text: says "Stop all dev services and database" but spec says "Stop postgres only"
- `make run` target uses `cargo run --bin moto-cli` but binary is named `moto`. Change to `cargo run --bin moto`.

---

## testing.md bug-fix

- Delete stale `// Run with: cargo test --features integration` comments in `moto-club-wg` sessions.rs:459 and peers.rs:345 (feature flag no longer exists)
- Rename `infra/smoke-test.sh` to `infra/smoke-test-garage.sh` and update Makefile `test-garage` target to match spec naming convention.

---

## keybox.md bug-fix

- Fix `POST /auth/issue-garage-svid` returning 401 instead of 403 for invalid service token: add `.map_err()` wrapper like other service-token-gated endpoints
- Fix `with_repository()` constructor hardcoding `admin_service` to `"moto-club"`: add `admin_service: &str` parameter to match `AppState::new()` constructor

---

## moto-club.md bug-fix

- Fix `state.k8s_client` always `None`: clone `K8sClient` before passing to `GarageK8s`, then chain `.with_k8s_client(k8s_client)` on `AppState` builder in `main.rs`
- Fix `set_session` not incrementing `peer_version`: add `wg_garage_repo::increment_peer_version` call after session creation in `postgres_stores.rs`
- Fix `peer_broadcaster` never called on session create/close: `create_session` and `close_session` never call `broadcast_add()` or `broadcast_remove()`, so garages connected via `WS /internal/wg/garages/{id}/peers` receive no events when sessions change
- Fix `close_session` spuriously incrementing `peer_version` when re-closing an already-closed session: `remove_session` now fetches raw DB session, checks `closed_at`, and only calls `close()` + `increment_peer_version` when the session is actually open
- Fix `close_session` idempotent re-close triggering spurious `broadcast_remove`: `remove_session` in `PostgresSessionStore` now returns `None` when session is already closed, so `SessionManager::close_session` returns `NotFound` and the handler skips `broadcast_remove`
- Fix `extend_ttl` max-TTL guard uses original `ttl_seconds` not actual total: compute `(expires_at + extension - created_at).num_seconds()` instead of `garage.ttl_seconds + req.seconds`
- Fallback name validation missing start/end alphanumeric + 63-char limit: `garages.rs` checks character set (lowercase + digits + hyphens) but doesn't enforce that name must start/end with alphanumeric or respect K8s 63-char label limit. Names like `-foo-` pass validation.
- close_session idempotent re-close returns 404 instead of 204: SessionManager converts None to NotFound, handler returns 404; fix to return Option<Session> and treat None as 204

---

## testing.md v0.7

- Confirmed `crates/moto-ai-proxy/tests/smoke_test.rs` already deleted with no leftover references in Cargo.toml or CI
- Created `infra/smoke-test-ai-proxy.sh` following keybox pattern: passthrough auth (200/401), path allowlist (403), unified endpoint routing (200/400), health endpoints (200), missing provider (503)

---

## makefile.md v0.19

- (spec-only) Fix `dev-down` description to "Stop postgres only"

## makefile.md v0.18

- (spec-only) Fix `push-garage` comment to include "clean up local copy"
- (spec-only) Document `registry-start` vs `REGISTRY` port mismatch with override guidance
- (spec-only) Document `deploy-system` port-forward side effect

## moto-bike.md bug-fix (2)

- Add `RUST_BACKTRACE="1"` to deployment builder `build_env_vars()` per spec.

## garage-lifecycle.md bug-fix

- Add `--name` CLI arg to `garage open` command and pass it through to `CreateGarageInput.name`.
- Add `--image` CLI arg to `garage open` command and pass it through to `CreateGarageInput.image`.

---

## service-deploy.md bug-fix

- Add security contexts to `club.yaml` matching `keybox.yaml` (runAsUser/runAsGroup/runAsNonRoot pod-level, readOnlyRootFilesystem/allowPrivilegeEscalation/capabilities container-level).
- Add metrics port 9090 to `keybox.yaml` Service and container port list per moto-bike.md spec.
- Replace static manifests with generated ones from bike.toml via `scripts/generate-manifests.sh`. Added `make generate-manifests` target. Generated manifests include deployment builder security baseline (rolling updates, full security contexts, common env vars).

## service-deploy.md bug-fix (2)

- Fix `scripts/generate-manifests.sh` single-quoted heredocs (`<< 'YAML'`) so `parse_toml` values are actually interpolated — replicas, resources now read from bike.toml. Added `parse_toml_section()` for TOML section-aware parsing. Fixed macOS sed compatibility.

## moto-cron.md v0.2

- Add TTL enforcement step to reconciler's reconcile_once(): call list_expired(limit 10), terminate each, delete K8s namespace
- Process expired garages oldest-first (ORDER BY expires_at ASC), at most 10 per cycle
- Log each termination: garage_id, garage_name, reason=ttl_expired
- On namespace deletion failure after DB termination, log warning and continue (orphan cleanup catches it next cycle)
- Continue to next expired garage on individual failure (don't fail the batch)

## moto-cron.md v0.3

- Add WHERE status != 'terminated' guard to garage_repo::terminate() to prevent overwriting concurrent user-initiated close
- Ensure TTL enforcement applies to all non-terminated states: Pending, Initializing, Ready, and Failed (verified via integration test)

## moto-club-websocket.md v0.2

- Implement log streaming WebSocket endpoint: /ws/v1/garages/{name}/logs with tail, follow, since query params
- Implement K8s pod log stream integration: historical lines first, then follow if requested, eof on pod terminate
- Implement event streaming WebSocket endpoint: /ws/v1/events with garages query param filter
- Implement TTL warning events in reconciler: emit ttl_warning at 15 and 5 minutes before expiry
- Implement status_change events on garage state transitions (from garage service and reconciler)
- Implement error events from reconciler (pod failures, crash loops)
- Update CLI to prefer WebSocket for log streaming, fall back to direct K8s API

## moto-club-websocket.md v0.3

- Add dropped message type for log backpressure: buffer up to 256 messages, drop oldest and notify client
- Add connection limits: max 5 concurrent log WS connections per garage, max 3 event WS connections per user
- Add owner-based auth (same as REST API) to log and event streaming endpoints
- Add garage state validation for log streaming: reject Pending and Terminated, allow Initializing/Ready/Failed
- Add reason field to status_change events on transitions to Terminated or Failed (values from TerminationReason enum)

## moto-wgtunnel.md v0.10

- Implement WebSocket client connection in daemon: read K8s SA token from `/var/run/secrets/kubernetes.io/serviceaccount/token`, connect to `peer_stream_url()` with Bearer auth, parse incoming PeerEvent JSON messages
- Implement reconnect logic: exponential backoff (1s, 2s, 4s, 8s, cap 30s) on WebSocket disconnect, log warning on each attempt, server re-sends full peer list on reconnect
- Replace `handle_peer_action()` placeholders: `PeerAction::Add` calls engine to add WireGuard peer with public_key + allowed_ip; `PeerAction::Remove` calls engine to remove peer by public_key
- Update health endpoint to reflect WebSocket connection status (`moto_club_connected`) and WireGuard tunnel status (`wireguard`)

## moto-cli.md v0.14

- Add `Watch` variant to `GarageAction` enum with `--garages` option (comma-separated names, optional)
- Add `stream_events_ws()` method to `MotoClubClient`: connect to `/ws/v1/events?garages=...` WebSocket, same auth pattern as `stream_logs_ws()`, return channel of parsed GarageEvent messages
- Implement `watch` command handler: connect via `stream_events_ws()`, format events for human output (e.g. `[garage-name] Status: From → To`), support `--json` for JSON Lines output (one event per line)
- Implement reconnect logic in watch: backoff (1s, 2s, 4s, cap 10s), fetch current state via REST on reconnect before resuming WebSocket

## ai-proxy.md v0.2

- Create moto-ai-proxy crate with binary entrypoint and config loading
- Add health endpoints (/health/live, /health/ready, /health/startup)
- Implement path-based provider routing (Anthropic, OpenAI, Gemini upstreams)
- Implement keybox secret injection via SVID authentication (spiffe://moto.local/service/ai-proxy)
- Add request size limit (10 MB max request body)
- Add request timeouts (connect 10s, first byte 30s, idle 120s, total 600s)
- Add bike engine deployment config (bike.toml, K8s Deployment/Service/ServiceAccount in moto-system)
- Implement garage identity validation via moto-club API
- Implement garage validation caching (default 60s)
- Add structured canonical logging with request_id, garage_id, provider, duration_ms
- Add configuration parsing for all MOTO_AI_PROXY_* env vars
- Implement streaming SSE pass-through (chunked transfer, flush immediately, no buffering)
- Implement API key caching with configurable TTL (default 5 min)

## ai-proxy.md v0.3

- Implement passthrough routes (/passthrough/anthropic/, /passthrough/openai/, /passthrough/gemini/)
- Implement unified endpoint (/v1/chat/completions) using OpenAI-compatible format
- Implement OpenAI → Anthropic request translation (messages, system message extraction, field mapping)
- Implement Anthropic → OpenAI non-streaming response translation
- Implement Anthropic → OpenAI streaming SSE response translation (event-by-event)
- Implement tool use translation between OpenAI and Anthropic formats
- Implement Gemini routing via OpenAI-compat mode (no translation, auth injection only)

## ai-proxy.md v0.4

- Add /v1/models endpoint returning merged model list from all configured providers
- Add MOTO_AI_PROXY_MODEL_MAP support for custom model prefix → provider mappings
- Implement model-based auto-routing: inspect model field and route to correct provider
- Support all provider keys stored simultaneously (no single-backend limitation)
- Remove MOTO_AI_PROXY_BACKEND env var (routing is automatic)
- Return 503 per-provider when a provider key is missing (other providers still work)

## ai-proxy.md v0.5

- Implement fine-tuned model name handling (strip ft: prefix before matching)
- Add X-Moto-Request-Id response header (correlation ID)
- Add X-Moto-Provider response header (provider that handled request)
- Implement passthrough path allowlist (block admin/billing endpoints, return 403 for disallowed paths)
- Use garage SVID for auth instead of predictable garage-{id} token
- Implement error sanitization: wrap all errors in OpenAI error format, scrub API key material
- Use SecretString (zeroize-on-drop) for cached API keys
- Implement local dev integration: ai-proxy in moto dev up startup sequence
- Support --no-ai-proxy flag for moto dev up
- Implement local dev key seeding (prompt or MOTO_DEV_*_KEY env vars)

## makefile.md v0.20

- Add `smoke-ai-proxy` target to Makefile (port-forward ai-proxy service, run smoke test script, kill port-forward on exit)

## testing.md v0.7

- Add `smoke-ai-proxy` Makefile target: port-forward ai-proxy, run `infra/smoke-test-ai-proxy.sh`, clean up

## moto-throttle v0.2

- Create moto-throttle crate with token bucket algorithm (capacity = burst, refill = RPM/60 tokens/sec, continuous refill)
- Implement ThrottleLayer as tower middleware that extracts principal and checks token bucket
- Implement principal extraction: JWT claim parsing from Authorization/x-api-key headers, service token detection, fallback to Unknown tier with client IP key
- Implement bucket cleanup: evict buckets not accessed within TTL (default 10 min), periodic sweep (default 60 sec)
- Support env var configuration (MOTO_THROTTLE_*_RPM, *_BURST, *_CLEANUP_INTERVAL_SECS, *_BUCKET_TTL_SECS)
- Implement rate limit tiers: garage (120 RPM, burst 20), bike (300, 50), service (1000, 100), unknown (30, 5)
- Implement per-endpoint path overrides (override_path config, 0 = no limit)
- Add response headers on all responses: X-RateLimit-Limit, X-RateLimit-Remaining, X-RateLimit-Reset
- Return 429 with JSON error body and Retry-After header when rate limited
- Read service token from MOTO_KEYBOX_SERVICE_TOKEN / MOTO_KEYBOX_SERVICE_TOKEN_FILE for service token detection
- Log warn on 429 with principal_id, principal_type, path, rpm_limit, retry_after_secs

## moto-throttle v0.3

- Handle malformed JWTs gracefully: invalid base64 or missing claims falls through to service token / unknown tier
- Ensure ThrottleLayer sits before auth layer in middleware stack ordering

## audit-logging v0.2

- Create shared audit event schema (id, event_type, service, principal_type, principal_id, action, resource_type, resource_id, outcome, metadata JSONB, client_ip, timestamp)
- Create audit_log table migration for moto-club database with indexes (timestamp, principal_id, event_type, resource_type+resource_id)
- Migrate keybox audit_log table to unified schema (map spiffe_id, secret_scope, secret_name to new fields; add service, action, resource_type, resource_id, outcome, metadata, client_ip columns)
- Implement AuditLogger for keybox: log secret_accessed, secret_created, secret_updated, secret_deleted, dek_rotated, svid_issued, auth_failed events
- Implement AuditLogger for moto-club: log garage_created, garage_terminated, garage_state_changed, ttl_enforced, auth_failed events from handlers and reconciler
- Implement ai-proxy structured audit log: emit newline-delimited JSON to stdout for ai_request, ai_request_denied, provider_error events (including token counts in metadata when available)
- Add audit log retention tasks to moto-cron reconciler (keybox 90 days, moto-club 30 days)
- Add GET /api/v1/audit/logs endpoint on moto-club with query filters (service, event_type, principal_id, resource_type, since, until, limit, offset)
- Auth: service token only for audit query endpoint (constant-time comparison, MOTO_CLUB_SERVICE_TOKEN_FILE env var)
- Ensure audit logging is best-effort: failures must not block primary operations
- Ensure sensitive data is never logged (secret values, API keys, tokens, request/response bodies)

## audit-logging v0.3

- Add keybox GET /audit/logs endpoint since/until query parameter support for fan-out queries from moto-club (pass-through time range filtering in DB layer and both in-memory/PG API handlers)
- Implement fan-out: moto-club queries own table and keybox /audit/logs in parallel, merges by timestamp, graceful degradation if keybox unreachable (AppState gets keybox_url and keybox_service_token fields; audit handler fans out to keybox with query param pass-through; response includes warnings field when keybox unavailable; ai-proxy returns informational warning)
