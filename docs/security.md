# Security

## Threat Model

Moto's security goal is **containment**: AI agents like Claude Code run with full autonomy inside garage containers, but a compromised garage cannot affect other garages, the control plane, or the host.

**The threat we defend against:**
- A malicious or compromised AI agent attempts to escape its garage
- An agent tries to access secrets or data from other garages
- An agent attempts to compromise the Kubernetes control plane
- An agent tries to pivot to the host or other infrastructure

**The philosophy:**
The container IS the security boundary. Root access inside the container is fine because the real perimeter is container namespaces, cgroups, seccomp profiles, and Kubernetes NetworkPolicies. We let AI agents run in YOLO mode inside the sandbox because the sandbox itself is hardened.

## Isolation Layers

Moto uses defense-in-depth with multiple overlapping controls:

### Container Security Context

Every garage pod runs with a hardened security context:

```yaml
securityContext:
  runAsUser: 0                      # Root inside container (enables apt install, dev tools)
  runAsGroup: 0
  allowPrivilegeEscalation: false   # Cannot gain additional privileges
  readOnlyRootFilesystem: true      # Immutable base, writes go to volumes
  seccompProfile:
    type: RuntimeDefault            # Blocks dangerous syscalls
  capabilities:
    drop:
      - ALL                         # Start with zero Linux capabilities
    add:
      - CHOWN                       # Minimal set needed for normal dev work
      - DAC_OVERRIDE
      - FOWNER
      - SETGID
      - SETUID
      - NET_BIND_SERVICE
```

**Why root inside is safe:**
- `allowPrivilegeEscalation: false` prevents gaining new capabilities
- `readOnlyRootFilesystem: true` makes the base image immutable
- `capabilities: drop ALL` removes dangerous Linux capabilities like `SYS_ADMIN`, `NET_ADMIN`, `SYS_PTRACE`
- Seccomp blocks syscalls like `mount`, `reboot`, `kexec_load`

Garages cannot:
- Mount filesystems
- Load kernel modules
- Access raw devices
- Escape namespaces
- Run truly privileged operations

### Network Policies

Each garage namespace has a default-deny NetworkPolicy:

**Ingress:** Deny all inbound connections (garages are not reachable from outside)

**Egress:** Allow only:
- DNS (port 53 to kube-dns in kube-system)
- Internet (for package installs, git clone, external APIs)
- Keybox (port 8080 in moto-system)
- Supporting services (postgres, redis in own namespace)

**Blocked egress:**
- moto-club (port 18080)
- Other garage namespaces
- Kubernetes API server (port 443 in default namespace)
- Cloud metadata endpoints (169.254.0.0/16)
- WireGuard overlay network
- Private IPv6 ranges (fd00::/8, ::1/128, fe80::/10)

Garages are isolated from each other and from control plane services.

### Kubernetes RBAC

Garages have **no Kubernetes API access**:

```yaml
automountServiceAccountToken: false
```

Garage pods cannot:
- List pods or secrets
- Create or delete resources
- Read ConfigMaps or other cluster state
- Escalate privileges via ServiceAccount tokens

### Resource Limits

Each garage namespace has ResourceQuota and LimitRange:

```yaml
ResourceQuota:
  limits.cpu: "4"
  limits.memory: "8Gi"
  persistentvolumeclaims: "2"

LimitRange:
  Container:
    max.cpu: "2"
    max.memory: "4Gi"
```

This prevents:
- Resource exhaustion attacks
- Noisy neighbor problems
- Runaway processes consuming all cluster resources

## Identity (SPIFFE SVIDs)

Moto uses SPIFFE-inspired identity tokens called **SVIDs** (SPIFFE Verifiable Identity Documents):

### How SVIDs Work

1. **Issuance:** When moto-club creates a garage, it requests an SVID from keybox:
   - Keybox signs an Ed25519 JWT with claims: `spiffe_id`, `pod_uid`, `principal_type`
   - TTL: 15 minutes for garages, 1 hour for bikes
   - Bound to the pod UID (prevents token reuse if pod is deleted and recreated)

2. **Authentication:** Garages and bikes present their SVID when calling keybox or ai-proxy:
   - The service validates the Ed25519 signature using keybox's public key
   - Checks the `exp` claim (reject expired tokens)
   - Verifies the `pod_uid` matches a running pod (garages only)

3. **No bearer token risk:** SVIDs are short-lived (15 min) and bound to pod UID. If a pod is deleted, its SVID becomes invalid immediately.

**SPIFFE ID format:**
- Garages: `spiffe://moto.local/garage/{garage_id}`
- Bikes: `spiffe://moto.local/bike/{bike_id}`

**Claims:**
```json
{
  "spiffe_id": "spiffe://moto.local/garage/g-abc123",
  "pod_uid": "12345678-abcd-1234-abcd-1234567890ab",
  "principal_type": "Garage",
  "exp": 1678886400
}
```

### SVID Lifecycle

- **Creation:** moto-club requests SVID from keybox during garage creation
- **Distribution:** SVID is pushed into the garage pod via Secret mount
- **Refresh:** Client libraries auto-refresh SVIDs before expiry (transparent to applications)
- **Revocation:** Deleting a pod invalidates its SVID (pod UID check fails)

## Secrets (Keybox)

Keybox is Moto's secrets manager. It stores API keys, database credentials, and other sensitive data.

### Envelope Encryption

Secrets are encrypted at rest using **envelope encryption**:

1. **Master key (KEK):** A 256-bit key stored in `/keys/master.key` (in production, use HSM/KMS)
2. **Data keys (DEK):** Each secret has its own 256-bit AES-GCM key
3. **Encryption:** DEK encrypts the secret value, KEK encrypts the DEK

**Why envelope encryption:**
- Rotating the master key only requires re-encrypting DEKs (not all secret values)
- Each secret has a unique DEK (limits blast radius of key compromise)
- DEKs can be rotated per-secret via `POST /admin/rotate-dek/{name}`

**Algorithm:** AES-256-GCM with random nonces (checked for uniqueness to prevent nonce reuse)

### Access Control (ABAC)

Keybox uses **Attribute-Based Access Control (ABAC)** to enforce who can access what:

**Policy rules:**
- **Garages** can read:
  - Global secrets (scope: `global`)
  - Service secrets for any service (scope: `service:*`)
  - Instance secrets for themselves (scope: `instance:{garage_id}`)
- **Garages** cannot read:
  - ai-proxy secrets (prevents bypassing credential injection)
  - Secrets in other instances
- **Bikes** can read:
  - Global secrets
  - Secrets for their own service only (scope: `service:{bike.service}`)
  - Instance secrets for themselves
- **Service tokens** (moto-club) can read and write all secrets (admin bypass)

**Enforcement:**
- Every secret request is checked against ABAC policy
- 403 Forbidden for both "not found" and "access denied" (prevents enumeration)

### Anti-Enumeration

Keybox prevents secret enumeration attacks:

- `GET /secret/{scope}/{name}` returns 403 for both "not found" and "no access"
- Listing secrets requires the same ABAC checks
- Timing attacks are mitigated by constant-time token comparison

## Network Boundaries

Garages can reach:
- **Internet** (HTTP/HTTPS) — for package installs, git clone, external APIs
- **DNS** (port 53) — for name resolution
- **Keybox** (port 8080 in moto-system) — for fetching secrets
- **Supporting services** — postgres and redis in the garage's own namespace

Garages **cannot** reach:
- **moto-club** (port 18080 in moto-system) — control plane is isolated
- **Other garages** — cross-namespace traffic is blocked
- **Kubernetes API server** — no ServiceAccount token is mounted
- **Cloud metadata endpoints** (169.254.0.0/16) — blocks AWS/GCP/Azure metadata APIs
- **WireGuard overlay** — terminal tunnels are point-to-point, not reachable from inside garages
- **Private IPv6 ranges** — fd00::/8, ::1/128, fe80::/10 are blocked

**Egress is allowlist-based:** Only explicitly allowed destinations are reachable.

## Compliance

Moto is designed for SOC 2 Type II compliance (Security, Availability, Confidentiality, Processing Integrity):

### Trust Service Criteria

| Control | Implementation |
|---------|---------------|
| **CC6 (Logical Access)** | SVID-based auth, ABAC policies, NetworkPolicy, SecurityContext, no K8s API access |
| **CC7 (System Operations)** | Audit logging, health checks, crash loop detection, TTL warnings |
| **CC8 (Change Management)** | Pre-commit hooks, CI pipeline, unit/integration tests, container signing |
| **A1 (Availability)** | Graceful degradation, ResourceQuota, rate limiting, orphan cleanup |
| **C1 (Confidentiality)** | Envelope encryption, WireGuard tunnels, secret scrubbing, anti-enumeration |
| **PI1 (Processing Integrity)** | State machine validation, input limits, idempotent operations, constant-time auth |

### Security Principles

All Moto code follows these principles:

1. **Defense in depth:** No single control is the only protection. Network isolation AND auth AND ABAC together.
2. **Least privilege:** Services mount only the secrets they need. Pods get only the capabilities they need.
3. **Cryptographic verification:** Identity claims MUST be cryptographically verified. Decoding a JWT without verifying the signature is NOT authentication.
4. **Audit everything:** All security-relevant operations produce audit events. Audit logging is best-effort (never blocks primary operations).
5. **Fail closed:** Auth failures, validation failures, and unreachable dependencies MUST deny access, not grant it.

### Audit Logging

Every security-relevant operation is logged:

- SVID issuance and validation
- Secret access (read, write, delete)
- DEK rotation
- Authentication failures
- Access denied events

Audit logs:
- Use a unified schema across all services (`moto-audit-types`)
- Are queryable via `GET /audit/logs` (service-token-only)
- Retain for 30 days (moto-club), 90 days (keybox)
- Sanitize sensitive data (16-pattern blocklist removes API keys, tokens, passwords)

### Deferred Items

The following are SOC 2 gaps but acceptable for initial deployments:

- **In-cluster TLS:** Pod-to-pod traffic is plaintext HTTP (requires service mesh)
- **HSM/KMS for master key:** File-based KEK is acceptable with compensating controls
- **Centralized log aggregation:** No ELK/Loki/Grafana pipeline yet
- **Tamper-evident audit logs:** No hash chaining or signed entries

See the Trust Service Criteria table above for the full SOC 2 control mapping.
