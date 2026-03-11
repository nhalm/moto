# Compliance

| | |
|--------|----------------------------------------------|
| Version | 0.2 |
| Status | Ready to Rip |
| Last Updated | 2026-03-11 |

## Overview

SOC 2 compliance requirements for the moto platform. This spec maps Trust Service Criteria to existing controls, identifies gaps, and defines what MUST be true for the system to be SOC 2 compliant.

**This spec is cross-cutting.** It does not own implementation — it references controls implemented in other specs. All specs MUST comply with the requirements defined here. When building new features, check this spec for applicable security requirements.

**Scope:** SOC 2 Type II (Security, Availability, Confidentiality, Processing Integrity). PCI DSS is deferred until the tokenization product layer is built.

## Security Principles

These apply to ALL specs and ALL code:

1. **Defense in depth** — No single control is the only protection. Network isolation AND auth AND ABAC together.
2. **Least privilege** — Services mount only the secrets they need. Pods get only the capabilities they need. RBAC grants only the permissions required.
3. **Cryptographic verification** — Identity claims MUST be cryptographically verified before trust. Decoding a JWT without verifying the signature is NOT authentication.
4. **Audit everything** — All security-relevant operations MUST produce audit events. Audit logging MUST be best-effort (never block primary operations).
5. **Fail closed** — Auth failures, validation failures, and unreachable dependencies MUST deny access, not grant it.

## SOC 2 Control Mapping

### CC6 — Logical Access Controls

**Requirements:**

| Control | Status | Owner Spec | Notes |
|---------|--------|------------|-------|
| SVID-based identity (Ed25519 JWT, 15min/1hr TTL) | Satisfied | keybox | Pod UID binding prevents replay |
| ABAC policy engine (per-scope, per-principal) | Satisfied | keybox | 15+ unit tests |
| Service token auth (constant-time comparison) | Satisfied | keybox | `subtle::ConstantTimeEq` |
| Secret enumeration prevention (403 for both not-found and denied) | Satisfied | keybox | |
| Garage pod isolation (SecurityContext, capabilities) | Satisfied | garage-isolation | `allowPrivilegeEscalation: false`, drop ALL caps |
| NetworkPolicy (deny-all ingress, scoped egress) | Satisfied (IPv4) | garage-isolation | **IPv6 gap — see Findings** |
| No K8s API access from garages | Satisfied | garage-isolation | `automountServiceAccountToken: false` |
| Cloud metadata blocked (169.254.0.0/16) | Satisfied | garage-isolation | |
| ResourceQuota / LimitRange per garage | Satisfied | garage-isolation | |
| SVID signature verification at all auth points | **CRITICAL GAP** | ai-proxy | ai-proxy decodes without verifying — see Findings |
| Token issuance requires authentication | **CRITICAL GAP** | keybox | `POST /auth/token` is unauthenticated — see Findings |
| Garage secret access scoped to own secrets | **HIGH GAP** | keybox | Garages can read all service-scoped secrets |
| K8s RBAC follows least privilege | **HIGH GAP** | service-deploy | ClusterRole has cluster-wide `secrets` access |
| Supporting service pods have no SA token | **GAP** | garage-isolation | postgres/redis pods mount SA token by default |

### CC7 — System Operations / Monitoring

| Control | Status | Owner Spec | Notes |
|---------|--------|------------|-------|
| Unified audit schema across all services | Satisfied | audit-logging | `AuditEvent` shared via `moto-audit-types` |
| Audit log query API with auth | Satisfied | audit-logging | Service-token-only, fan-out to keybox |
| moto-club audit retention (30 days, batched) | Satisfied | audit-logging | Reconciler step 7 |
| Health check endpoints | Satisfied | moto-bike | `/health/live`, `/health/ready`, `/health/startup` |
| Crash loop detection | Satisfied | moto-club | WebSocket events on CrashLoopBackOff |
| TTL warning events | Satisfied | moto-club | 15min and 5min warnings, deduplicated |
| Sensitive data sanitization in audit events | Satisfied | audit-logging | 16 pattern blocklist, enforced at build() |
| Keybox 90-day retention | **GAP** | audit-logging | Not yet in moto-cron reconciler |
| ai-proxy audit events queryable | **GAP** | audit-logging | stdout-only, not in query API |
| Centralized log aggregation / SIEM | **GAP** | (new work needed) | No ELK/Loki/Grafana pipeline |
| Anomaly detection / alerting | **GAP** | (new work needed) | No rules for rapid auth failures etc. |
| Tamper-evidence on audit log | **GAP** | audit-logging | No hash chaining or signed entries |

### CC8 — Change Management

| Control | Status | Owner Spec | Notes |
|---------|--------|------------|-------|
| Pre-commit hooks (fmt, clippy, secret file blocking) | Satisfied | pre-commit | Blocks .pem, .key, .env files |
| `make ci` target | Satisfied | makefile | fmt + check + lint + test |
| Unit test suite | Satisfied | testing | Co-located with implementation |
| Integration test infrastructure | Satisfied | testing | docker-compose, smoke tests |
| Secret content scanning in pre-commit | **GAP** | pre-commit | Hook only checks file names, not contents |
| CI/CD pipeline (GitHub Actions) | **GAP** | (new work needed) | No automated pipeline blocking merges |
| Dependency vulnerability scanning (`cargo audit`) | **GAP** | pre-commit / makefile | Known CVEs not detected |
| Container image signing / SBOM | **GAP** | container-system | No Cosign, no SLSA provenance |

### A1 — Availability

| Control | Status | Owner Spec | Notes |
|---------|--------|------------|-------|
| Graceful health degradation (degraded ≠ 503) | Satisfied | moto-club | K8s/keybox down → 200 degraded |
| TTL enforcement with rate limiting | Satisfied | moto-cron | 10 per reconcile cycle |
| ResourceQuota / LimitRange | Satisfied | garage-isolation | Prevents resource exhaustion |
| Best-effort audit logging | Satisfied | audit-logging | Never blocks primary operations |
| Orphan cleanup (K8s/DB drift) | Satisfied | moto-cron | Reconciler detects and cleans orphans |
| SVID auto-refresh in client library | Satisfied | keybox | Transparent refresh before expiry |
| Rate limiting middleware | Satisfied | moto-throttle | Token bucket, per-principal tiers |
| PodDisruptionBudgets | **GAP** | service-deploy | Rolling updates can cause downtime |
| Leader election for reconciler | **GAP** | moto-club | Multi-replica runs double-fire reconciliation |

### C1 — Confidentiality

| Control | Status | Owner Spec | Notes |
|---------|--------|------------|-------|
| Envelope encryption (AES-256-GCM, KEK/DEK) | Satisfied | keybox | Correct nonce handling, tested |
| DEK rotation | Satisfied | keybox | `POST /admin/rotate-dek/{name}` |
| `Secret<T>` wrapper (Debug → REDACTED) | Satisfied | keybox | Used throughout |
| API key scrubbing in error responses | Satisfied | ai-proxy | `scrub_api_keys()` with pattern matching |
| WireGuard tunnel encryption | Satisfied | moto-wgtunnel | Client-to-garage transit |
| Pre-commit blocks secret files | Satisfied | pre-commit | .pem, .key, .env |
| In-cluster TLS between services | **GAP** | service-deploy | Plaintext HTTP between pods |
| HSM/KMS for master key | **GAP** | keybox | File-based KEK |
| Master key versioning | **GAP** | keybox | Single active KEK, no rollback |

### PI1 — Processing Integrity

| Control | Status | Owner Spec | Notes |
|---------|--------|------------|-------|
| Garage lifecycle state machine (tested) | Satisfied | garage-lifecycle | Exhaustive match, 15 tests |
| Input validation (1MB secret limit, TTL bounds) | Satisfied | keybox, moto-club | |
| Idempotent reconciliation | Satisfied | moto-cron | DB-level guards |
| Constant-time token comparison | Satisfied | keybox, audit-logging | `subtle::ConstantTimeEq` |
| Deterministic SVID validation | Satisfied | keybox | Signature + expiry + issuer + audience |

## Findings — Critical and High Priority

### CRITICAL-1: Unauthenticated SVID Issuance

**Spec:** keybox
**Location:** `crates/moto-keybox/src/api.rs` — `POST /auth/token`
**Issue:** Any caller that can reach keybox can mint a valid SVID for any principal identity (garage, bike, service). No authentication required.
**Impact:** Complete identity spoofing. A compromised garage could mint SVIDs for other garages.
**Fix:** Require service token authentication on `POST /auth/token`. Only moto-club should be able to issue SVIDs.

### CRITICAL-2: ai-proxy Does Not Verify SVID Signatures

**Spec:** ai-proxy
**Location:** `crates/moto-ai-proxy/src/auth.rs` — `extract_garage_id()`
**Issue:** JWT claims are decoded (base64) but the Ed25519 signature is never verified. A structurally valid JWT with any `principal_id` passes auth if the garage exists in moto-club.
**Impact:** Any network-reachable party can impersonate any garage for AI requests.
**Fix:** Verify the Ed25519 signature using keybox's public verifying key. ai-proxy needs the verifying key (not the signing key) at startup.

### HIGH-1: IPv6 NetworkPolicy Gap

**Spec:** garage-isolation
**Location:** `crates/moto-club-k8s/src/network_policy.rs`
**Issue:** All NetworkPolicy rules are IPv4-only (`0.0.0.0/0`). On a dual-stack cluster, IPv6 egress is completely uncontrolled. The WireGuard overlay uses IPv6 (`fd00:moto::/48`), which is unblocked.
**Impact:** Garages could reach other garages via IPv6, bypassing all isolation.
**Fix:** Add IPv6 egress rules mirroring IPv4 rules. Block `fd00::/8` (ULA), `::1/128` (loopback), `fe80::/10` (link-local).

### HIGH-2: Garage ABAC Too Broad for Service Secrets

**Spec:** keybox
**Location:** `crates/moto-keybox/src/abac.rs` — `evaluate_service()`
**Issue:** Garages can read ALL service-scoped secrets (e.g., `ai-proxy/anthropic`), bypassing ai-proxy's credential injection and audit trail.
**Impact:** Garages can directly fetch API keys that should only be injected by ai-proxy.
**Fix:** Restrict garage access to service secrets. Options: deny-list for sensitive service prefixes, or require explicit grant.

### HIGH-3: moto-club ClusterRole Over-Scoped

**Spec:** service-deploy
**Location:** `infra/k8s/moto-system/club.yaml`
**Issue:** ClusterRole grants `get, list, create, delete` on `secrets` cluster-wide. moto-club could read `keybox-keys` (master key) from `moto-system`.
**Impact:** Compromised moto-club = compromised master key = all secrets exposed.
**Fix:** Scope secrets access to `moto-garage-*` namespaces only (namespace-scoped Roles created per garage), or add a deny rule for `moto-system`.

### HIGH-4: Supporting Service Pods Have SA Token

**Spec:** garage-isolation
**Location:** `crates/moto-club-k8s/src/supporting_services.rs`
**Issue:** Per-garage postgres and redis Deployments don't set `automountServiceAccountToken: false`. These pods get a default SA token.
**Impact:** If postgres/redis is compromised (e.g., via SQL injection from the garage), the attacker gets a K8s API token.
**Fix:** Add `automount_service_account_token: Some(false)` to postgres and redis pod specs.

## Deferred Items

These are real SOC 2 gaps but are not blocking for initial compliance posture:

- **In-cluster TLS** — Requires service mesh (Linkerd/Istio) or per-service TLS config. Significant infrastructure work.
- **HSM/KMS for master key** — Cloud-provider dependent. File-based KEK is acceptable for initial audit with compensating controls.
- **Master key versioning** — Needed for KEK rotation without downtime. Complex; acceptable to defer with documented rotation procedure.
- **Centralized log aggregation / SIEM** — Requires Loki/ELK/Datadog. Infrastructure decision.
- **Anomaly detection / alerting** — Depends on SIEM. Define alert rules after aggregation is in place.
- **Tamper-evident audit log** — Hash chaining or append-only DB role. Can add incrementally.
- **CI/CD pipeline** — GitHub Actions workflow for automated merge blocking.
- **Container image signing / SBOM** — Cosign + SLSA provenance. Depends on CI/CD.
- **PodDisruptionBudgets** — Add when running multi-replica in production.
- **Leader election for reconciler** — Needed when moto-club runs >1 replica. Use K8s Lease API.
- **Dependency vulnerability scanning** — Add `cargo audit` to CI and/or pre-commit.
- **Pre-commit content scanning** — Add `gitleaks` or `detect-secrets` for secret pattern matching.

## References

- [keybox.md](keybox.md) — Encryption, SVID, ABAC, secret management
- [garage-isolation.md](garage-isolation.md) — NetworkPolicy, SecurityContext, resource limits
- [audit-logging.md](audit-logging.md) — Audit trails, retention, query API
- [ai-proxy.md](ai-proxy.md) — AI gateway auth and credential injection
- [service-deploy.md](service-deploy.md) — K8s deployment, RBAC, secrets scoping
- [pre-commit.md](pre-commit.md) — Git hooks, code quality gates
- [moto-throttle.md](moto-throttle.md) — Rate limiting
- [moto-cron.md](moto-cron.md) — TTL enforcement, reconciliation

## Changelog

### v0.2 (2026-03-11)
- Full SOC 2 control mapping (CC6, CC7, CC8, A1, C1, PI1)
- Security audit findings: 2 critical, 4 high priority
- Deferred items list for incremental compliance improvement
- Cross-cutting security principles

### v0.1 (2026-01-19)
- Initial placeholder
