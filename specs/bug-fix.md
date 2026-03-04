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

(none)

## testing.md

- `moto-club-wg`: stale `// Run with: cargo test --features integration` comments in `sessions.rs:459` and `peers.rs:345` reference a feature flag that no longer exists (removed from `Cargo.toml`). Delete the misleading comments.

## moto-club.md

- **`state.k8s_client` is always `None` — K8s SA token validation permanently bypassed.** `main.rs:266-270` creates `K8sClient` and consumes it into `GarageK8s`, but `AppState` (built at `main.rs:320-328`) never calls `.with_k8s_client()`. Result: `validate_garage_token` in `wg.rs:363-367` short-circuits to `Ok(())` for all requests, and `/health/ready` in `health.rs:214-223` reports K8s as `"ok"` without checking. Fix: clone the client via `garage_k8s.client()` (accessor at `lib.rs:93`) and pass to `.with_k8s_client()`.

## keybox.md

- **`POST /auth/token` ignores `MOTO_KEYBOX_SVID_TTL_SECONDS`.** `api.rs:641` and `pg_api.rs:325` construct `SvidClaims::new(&spiffe_id, DEFAULT_SVID_TTL_SECS)` with hardcoded 900s instead of using the issuer's configured TTL. Setting the env var has no effect on issued SVIDs.
- **ABAC service global-secret prefix check too broad.** `abac.rs:149-152` has `|| secret.name.starts_with(&claims.principal_id)` without trailing slash. A service named `ai` gets access to secrets prefixed `ai-proxy/`. Fix: remove the second `||` branch or require the trailing slash.

## dev-container.md

(none)

## container-system.md

(none)

## local-cluster.md

(none)

## garage-isolation.md

(none)

## garage-lifecycle.md

(none)

## moto-bike.md

(none)

## supporting-services.md

(none)

## moto-wgtunnel.md

(none)

## local-dev.md

(none)

## service-deploy.md

(none)
