# Keybox

| | |
|--------|----------------------------------------------|
| Version | 0.11 |
| Status | Ready to Rip |
| Last Updated | 2026-03-02 |

## Changelog

### v0.11 (2026-03-02)
- Add test requirements for auth matrix enforcement and DEK rotation.

### v0.10 (2026-03-02)
- Enforce endpoint authorization matrix: `set_secret` and `delete_secret` must require service token (deny SVID with 403). `get_secret` and `list_secrets` must accept both service token and SVID. `get_audit_logs` must accept service token directly (not just SVID with Service principal type).
- Implement `POST /admin/rotate-dek/{name}`: rotates DEK for a secret, re-encrypts value, creates new version. Service token only. New `dek_rotated` audit event type.
- Move DEK rotation out of Future Work (Phase 2) — now implemented.

### v0.9 (2026-02-25)
- Add missing env vars to Configuration section: `MOTO_KEYBOX_SERVICE_TOKEN_FILE`, `MOTO_KEYBOX_BIND_ADDR`, `MOTO_KEYBOX_HEALTH_BIND_ADDR`

### v0.8 (2026-02-24)
- Fix: Secret retrieval handlers must enforce pod UID binding — API handlers (`get_secret`, `set_secret`, `delete_secret`) must call `validate_with_pod_uid()` instead of `validate()` when the SVID contains a `pod_uid` claim (spec: "Checks pod UID matches (still alive)" in Secret Retrieval Flow step 2)

### v0.7 (2026-02-24)
- Document that `moto keybox init` only generates `master.key` and `signing.key`, not `service-token`
- Add note on separate service-token generation

### v0.6 (2026-02-06)
- Add tests per [testing.md](testing.md) architecture

### v0.5 (2026-02-04)
- **BREAKING:** Rename `moto-keybox-server` binary from `moto-keybox` to `moto-keybox-server`
  - Fixes cargo doc collision with `moto-keybox` library crate
  - Update Cargo.toml: `[[bin]] name = "moto-keybox-server"`
- moto-club health check integration: `/health/ready` should check keybox availability
  - moto-club calls keybox `/health/ready` endpoint
  - Returns degraded health if keybox unreachable

### v0.4 (2026-02-04)
- Wire up moto-keybox-db PostgreSQL backend for secrets and audit logs (was in-memory only)
- Add 1 MB maximum secret size limit (API validation)
- Return 403 Forbidden for both "not found" and "access denied" (prevents secret enumeration)
- Fix bikes ABAC: enforce service field matching (bikes can only read their own service's secrets)
- Add health check endpoints per moto-bike.md spec (/health/live, /health/ready, /health/startup on port 8081)
- Add request logging/metrics middleware (Phase 2)
- Add key rotation mechanism (Phase 2)

### v0.3 (2026-02-02)
- Added POST /auth/issue-garage-svid endpoint for moto-club delegation
- Garages no longer use K8s ServiceAccount (SVID pushed by moto-club)
- Separated auth flows for bikes (K8s SA) vs garages (moto-club delegation)
- Added endpoint authorization matrix (SVID vs service token access)

### v0.2 (2026-01-26)
- Clarified keybox is internal service (users go through moto-club)
- Added service-to-service auth section (moto-club → keybox)
- Added policy storage note (hardcoded for MVP)
- Added local development section (dev SVID workflow)
- Added CLI commands section

### v0.1 (2026-01-19)
- Initial spec

## Overview

Secrets manager for moto. Provides credentials to garages (wrenching) and bikes (ripping) without baking secrets into containers or code. Uses SPIFFE-inspired identity for authentication and ABAC for authorization.

**Keybox is an internal service.** It is not publicly exposed. All user-facing secret management goes through **moto-club**, which handles user authentication and proxies requests to keybox. Garages and bikes authenticate directly to keybox via SVID (they're inside the cluster).

## Specification

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                           Keybox                                │
│                                                                 │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  │
│  │  moto-keybox    │  │  moto-keybox    │  │  moto-keybox    │  │
│  │  (server)       │  │  (client lib)   │  │  (cli)          │  │
│  │                 │  │                 │  │                 │  │
│  │  - Auth         │  │  - SVID cache   │  │  - Secret CRUD  │  │
│  │  - SVID issue   │  │  - Auto refresh │  │  - Key mgmt     │  │
│  │  - Secret store │  │  - Fetch API    │  │  - Audit view   │  │
│  │  - ABAC engine  │  │                 │  │                 │  │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

**Components:**

| Crate | Purpose |
|-------|---------|
| `moto-keybox` | Server: auth, SVID issuance, secret storage, ABAC |
| `moto-keybox-client` | Library: garages/bikes use to fetch secrets |
| `moto-keybox-cli` | CLI: manage secrets, view audit logs |

### SPIFFE-Inspired Identity

Not full SPIRE (too complex). Implement subset needed for moto.

**SPIFFE ID Format:**
```
spiffe://moto.local/garage/{garage-id}
spiffe://moto.local/bike/{bike-id}
spiffe://moto.local/service/{service-name}
```

**SVID (Short-lived identity token):**
- JWT signed by keybox
- 15-minute TTL
- Bound to pod UID (prevents replay if pod dies)
- Contains: SPIFFE ID, scope claims, pod metadata

### Secret Scoping

Secrets exist at three levels, checked in order:

| Scope | Description | Example |
|-------|-------------|---------|
| **Instance** | Per-garage or per-bike | Ephemeral dev credentials |
| **Service** | Per-engine/service type | `moto-tokenization` DB password |
| **Global** | Platform-wide | Master encryption keys |

Resolution priority: Instance → Service → Global

### Authentication Flow

**Bikes (have K8s ServiceAccount):**

```
1. Bike pod starts with K8s ServiceAccount JWT

2. Pod → POST /auth/token
   Headers: Authorization: Bearer <K8s SA JWT>

   Keybox:
   - Validates JWT via K8s TokenReview API
   - Fetches pod metadata via K8s API
   - Verifies pod labels (bike-id, etc.)
   - Signs SVID JWT with Ed25519 key

   ← Returns: Signed SVID (15 min TTL)

3. Pod caches SVID, refreshes at 14 min (before expiry)
```

**Garages (no K8s ServiceAccount - SVID pushed by moto-club):**

Garages don't have K8s API access (`automountServiceAccountToken: false`). Instead, moto-club requests an SVID on behalf of the garage and pushes it via Secret.

```
1. moto-club creates garage

2. moto-club → POST /auth/issue-garage-svid
   Headers: Authorization: Bearer <service-token>
   Body: { "garage_id": "abc123", "owner": "user@example.com" }

   Keybox:
   - Validates moto-club service token
   - Creates SVID for garage identity
   - Signs SVID JWT with Ed25519 key

   ← Returns: Signed SVID (1 hour TTL)

3. moto-club creates Secret in garage namespace with SVID

4. Garage pod mounts SVID Secret, uses for keybox requests

5. moto-club refreshes SVID before expiry, updates Secret
```

### Service-to-Service Auth (moto-club)

moto-club is the public API gateway. It handles user authentication (Authentik/OIDC in the future) and proxies secret management requests to keybox.

**Auth mechanism:** Static shared token for MVP. moto-club includes this token in requests to keybox.

```
moto-club → POST /secrets/global/ai/anthropic
  Headers: Authorization: Bearer <service-token>
```

Future: mTLS or SPIFFE-based service identity.

### Secret Retrieval Flow

```
1. Pod → GET /secrets/{scope}/{name}
   Headers: Authorization: Bearer <SVID>

2. Keybox:
   - Validates SVID signature
   - Checks SVID not expired
   - Checks pod UID matches (still alive)
   - Evaluates ABAC policy
   - Decrypts secret (envelope decryption)
   - Logs access event (no value logged)

   ← Returns: Secret value

3. Pod holds secret in SecretString (zeroizes on drop)
```

### Encryption Model

**Envelope Encryption:**

```
┌─────────────────────────────────────────────────┐
│  Master Key (KEK)                               │
│  - Loaded from env/file at startup              │
│  - Never persisted in database                  │
│  - Future: HSM/KMS backend                      │
└─────────────────────────────────────────────────┘
           │
           │ encrypts
           ▼
┌─────────────────────────────────────────────────┐
│  Data Encryption Keys (DEKs)                    │
│  - One per secret                               │
│  - Random AES-256 key                           │
│  - Stored encrypted in DB                       │
└─────────────────────────────────────────────────┘
           │
           │ encrypts
           ▼
┌─────────────────────────────────────────────────┐
│  Secret Values                                  │
│  - Encrypted with DEK using AES-256-GCM         │
│  - Stored as ciphertext in DB                   │
└─────────────────────────────────────────────────┘
```

**Database theft = useless** without the KEK.

### Access Control (ABAC)

Attribute-Based Access Control evaluates:

**Principal attributes (from SVID):**
- `type`: garage | bike | service
- `id`: garage-id, bike-id, or service name
- `pod_namespace`
- `pod_name`

**Resource attributes (from secret):**
- `scope`: global | service | instance
- `service`: which service it belongs to
- `instance_id`: garage-id or bike-id if instance-scoped

**Example policies:**
```
# Bike can access its own instance secrets
principal.type == "bike" AND
principal.id == resource.instance_id AND
resource.scope == "instance"

# Bike can access its service's secrets
principal.type == "bike" AND
principal.service == resource.service AND
resource.scope == "service"

# AI proxy can access global AI keys
principal.type == "service" AND
principal.id == "ai-proxy" AND
resource.scope == "global" AND
resource.name STARTS_WITH "ai/"
```

**Policy storage:** Policies are hardcoded in Rust for MVP (garage accesses garage secrets, bike accesses bike secrets, etc.). Future: load from config file for flexibility.

### Secret Types

| Type | Scope | Examples |
|------|-------|----------|
| AI API keys | Global | `ai/anthropic`, `ai/openai`, `ai/gemini` |
| Database credentials | Service | `db/tokenization/password` |
| Encryption keys | Global | `crypto/master-key` |
| Dev credentials | Instance (garage) | `dev/github-token` |
| Service tokens | Instance (bike) | Per-bike auth tokens |

### Zero-Trust Principles

1. **Network distrust** - TLS required, no IP-based trust
2. **Short credentials** - 15 min SVID TTL, pod UID binding
3. **Least privilege** - Secrets scoped tightly, default deny
4. **Pull-based** - Pods fetch secrets, nothing injected
5. **Audit everything** - All access logged, no values in logs

### Client Library Usage

```rust
// In a garage or bike
let client = KeyboxClient::new()?;

// Fetches SVID automatically using K8s SA JWT
// Caches and refreshes SVID transparently
let api_key = client.get_secret(Scope::Global, "ai/anthropic").await?;

// Use the secret
let value = api_key.expose();

// Automatic zeroization when api_key drops
```

### Local Development

In K8s, pods authenticate via ServiceAccount JWT. For local development without K8s, use CLI-issued dev SVIDs.

**Setup:**
```bash
# Issue a dev SVID for local garage testing
moto keybox issue-dev-svid --garage-id=test-garage --output=./dev-svid.jwt

# SVID is long-lived (24h) for dev convenience
```

**Client usage:**
```rust
// Client detects local mode via env var
// MOTO_KEYBOX_SVID_FILE=./dev-svid.jwt

let client = KeyboxClient::new()?;  // Reads SVID from file instead of K8s
let secret = client.get_secret(Scope::Global, "ai/anthropic").await?;
```

The client library supports multiple modes:
- **K8s mode:** Fetches SVID via ServiceAccount JWT (bikes)
- **File mode:** Reads SVID from `MOTO_KEYBOX_SVID_FILE` (garages, local dev)
- **Local mode:** Alias for file mode with dev SVID

### API Endpoints

**Authentication:**
```
POST /auth/token
  Request: K8s ServiceAccount JWT (for bikes)
  Response: Signed SVID (15 min TTL)

POST /auth/issue-garage-svid
  Auth: Service token (moto-club only)
  Request: { "garage_id": "...", "owner": "..." }
  Response: Signed SVID (1 hour TTL)
```

**Secrets:**
```
GET  /secrets/{scope}/{name}     - Retrieve secret
POST /secrets/{scope}/{name}     - Create/update secret
DELETE /secrets/{scope}/{name}   - Delete secret
GET  /secrets/{scope}            - List secrets in scope (names only, no values)
```

**Admin:**
```
GET  /audit/logs                 - Query audit logs
POST /admin/rotate-dek/{name}    - Rotate a secret's DEK
```

### Endpoint Authorization

Keybox enforces logical isolation based on token type:

| Endpoint | SVID (garages/bikes) | Service Token (moto-club) |
|----------|---------------------|---------------------------|
| `POST /auth/token` | No (uses K8s SA JWT) | No |
| `POST /auth/issue-garage-svid` | **Denied** | Allowed |
| `GET /secrets/{scope}/{name}` | Allowed (ABAC checked) | Allowed |
| `POST /secrets/{scope}/{name}` | **Denied** | Allowed |
| `DELETE /secrets/{scope}/{name}` | **Denied** | Allowed |
| `GET /secrets/{scope}` | Allowed (own scope only) | Allowed (all scopes) |
| `GET /audit/logs` | **Denied** | Allowed |
| `POST /admin/*` | **Denied** | Allowed |

**Enforcement rules (both `api.rs` and `pg_api.rs`):**

| Handler | Auth Logic |
|---------|-----------|
| `set_secret` | `validate_service_token()` only. Deny with `403 FORBIDDEN "Operation requires service token"` if not a service token. |
| `delete_secret` | Same as `set_secret`. |
| `get_secret` | Try `validate_service_token()` first (skip ABAC if OK). Otherwise `extract_svid_enforcing_pod_uid()` + ABAC. |
| `list_secrets` | Try `validate_service_token()` first (return all in scope). Otherwise `extract_svid()` + ABAC (own scope). |
| `get_audit_logs` | `validate_service_token()` only. Deny with `403 FORBIDDEN`. |
| `rotate_dek` | `validate_service_token()` only. Deny with `403 FORBIDDEN`. |
| `issue_garage_svid` | `validate_service_token()` only (already implemented). |

Error code for auth failures: `FORBIDDEN` (new code, distinct from `ACCESS_DENIED` used by ABAC).

### Admin Endpoints

#### POST /admin/rotate-dek/{name}

Rotates the DEK for a secret. The secret value does not change — it is decrypted with the old DEK and re-encrypted with a new DEK.

**Auth:** Service token required.

**Path:** `{name}` is the secret name within the scope. The scope is provided as a query parameter: `?scope=global` or `?scope=service/club` or `?scope=instance/{id}`.

**Request body:** None.

**Response (200):**
```json
{
  "name": "api-key",
  "scope": "global",
  "version": 3,
  "rotated_at": "2026-03-02T12:00:00Z"
}
```

**Errors:**
- `403 FORBIDDEN` — not a service token
- `404 SECRET_NOT_FOUND` — secret doesn't exist
- `500 INTERNAL_ERROR` — encryption or DB failure

**Steps:**
1. Validate service token
2. Look up secret by scope + name
3. Fetch current version with encrypted value and DEK
4. Decrypt: unwrap old DEK with master key, decrypt ciphertext
5. Generate new DEK
6. Re-encrypt value with new DEK
7. Wrap new DEK with master key
8. Store new encrypted DEK in `encrypted_deks`
9. Create new `secret_versions` row (incremented version, new ciphertext, new dek_id)
10. Update secret's `current_version` and `updated_at`
11. Log `dek_rotated` audit event
12. Return result

**Audit:** New `dek_rotated` event type (add to `AuditEventType` enum in `moto-keybox/src/types.rs` and `moto-keybox-db/src/models.rs`).

### CLI Commands

The CLI (`moto-keybox-cli`) is for **local development and admin tasks only**. In production, users manage secrets via moto-club UI.

**Initialization:**
```bash
# Generate KEK and SVID signing key (run once)
moto keybox init --output-dir=./keybox-keys

# Creates:
#   ./keybox-keys/master.key      (KEK, AES-256, base64-encoded)
#   ./keybox-keys/signing.key     (Ed25519 private key)
```

**Note:** `moto keybox init` generates `master.key` and `signing.key` only. The `service-token` (a random hex string used for moto-club → keybox auth) is generated separately, e.g., `openssl rand -hex 32 > service-token`. See [local-dev.md](local-dev.md) for the full local dev key generation flow.

**Secret management (local dev):**
```bash
# Set a secret (requires keybox server running)
moto keybox set global ai/anthropic "sk-ant-..." --url http://localhost:8080

# Read from stdin to avoid shell history
echo "sk-ant-..." | moto keybox set global ai/anthropic --stdin

# List secrets
moto keybox list global

# Get a secret
moto keybox get global ai/anthropic
```

**Dev SVID issuance:**
```bash
# Issue dev SVID for local garage testing
moto keybox issue-dev-svid --garage-id=test-garage --output=./dev-svid.jwt
```

### Configuration

```bash
# Required
MOTO_KEYBOX_MASTER_KEY_FILE="/run/secrets/keybox-master-key"
MOTO_KEYBOX_SVID_SIGNING_KEY_FILE="/run/secrets/svid-signing-key"
MOTO_KEYBOX_DATABASE_URL="postgres://keybox:password@localhost:5432/keybox"

# Optional
MOTO_KEYBOX_SVID_TTL_SECONDS="900"              # Default 15 min
MOTO_KEYBOX_SERVICE_TOKEN_FILE="/run/secrets/service-token"  # Alternative to MOTO_KEYBOX_SERVICE_TOKEN env var
MOTO_KEYBOX_BIND_ADDR="0.0.0.0:8080"            # Default 0.0.0.0:8080
MOTO_KEYBOX_HEALTH_BIND_ADDR="0.0.0.0:8081"     # Default 0.0.0.0:8081
```

### Database Schema (PostgreSQL)

```sql
-- Secrets metadata
CREATE TABLE secrets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope TEXT NOT NULL,        -- global, service, instance
    service TEXT,               -- null for global
    instance_id TEXT,           -- null for global/service
    name TEXT NOT NULL,
    current_version INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ,     -- soft delete
    UNIQUE(scope, service, instance_id, name)
);

-- Secret versions (encrypted values)
CREATE TABLE secret_versions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    secret_id UUID NOT NULL REFERENCES secrets(id),
    version INTEGER NOT NULL,
    ciphertext BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    dek_id UUID NOT NULL REFERENCES encrypted_deks(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(secret_id, version)
);

-- Encrypted DEKs
CREATE TABLE encrypted_deks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    encrypted_key BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Audit log
CREATE TABLE audit_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type TEXT NOT NULL,   -- accessed, created, deleted, etc.
    principal_type TEXT,
    principal_id TEXT,
    spiffe_id TEXT,
    secret_scope TEXT,
    secret_name TEXT,
    timestamp TIMESTAMPTZ NOT NULL DEFAULT now()
    -- NO secret values ever logged
);

-- Indexes
CREATE INDEX idx_secrets_scope ON secrets(scope);
CREATE INDEX idx_secrets_service ON secrets(service) WHERE service IS NOT NULL;
CREATE INDEX idx_audit_log_timestamp ON audit_log(timestamp);
CREATE INDEX idx_audit_log_spiffe_id ON audit_log(spiffe_id);
```

### Secret Size Limits

To prevent memory exhaustion and DoS attacks, keybox enforces a maximum secret size:

| Limit | Value |
|-------|-------|
| Maximum secret value size | 1 MB (1,048,576 bytes) |

The API returns `400 Bad Request` with error code `SECRET_TOO_LARGE` if the decoded secret value exceeds this limit.

### Error Response Consistency (Enumeration Prevention)

To prevent attackers from enumerating which secrets exist, keybox returns the same HTTP status code for both "secret not found" and "access denied":

| Condition | HTTP Status | Error Code |
|-----------|-------------|------------|
| Secret does not exist | 403 Forbidden | ACCESS_DENIED |
| Secret exists but access denied | 403 Forbidden | ACCESS_DENIED |

This prevents information leakage where different response codes could reveal secret existence.

### Bikes ABAC: Service Field Enforcement

Bikes must only access secrets belonging to their own service. The SVID for bikes includes a `service` claim.

**Updated policy (replaces MVP "bikes can read any service secret"):**
```
# Bike can access its service's secrets (ENFORCED)
principal.type == "bike" AND
principal.service == resource.service AND
resource.scope == "service"
```

The bike's service is determined from:
1. The `service` claim in the bike's SVID (required for bikes)
2. The `moto.dev/service` label on the bike pod

Bikes without a service claim cannot access service-scoped secrets.

### Health Check Endpoints

Per moto-bike.md Engine Contract, keybox exposes health endpoints on port 8081:

| Endpoint | Returns 200 when |
|----------|------------------|
| `GET /health/live` | Process is alive (not deadlocked) |
| `GET /health/ready` | Ready for traffic (master key loaded, DB connected) |
| `GET /health/startup` | Initial startup complete |

**Readiness criteria:**
- Master key successfully loaded
- SVID signing key successfully loaded
- Database connection established (when using PostgreSQL backend)

### Test Requirements

Per [testing.md](testing.md): handler tests use mocks (unit), database tests hit real PostgreSQL (integration).

**Auth matrix enforcement:**

- `set_secret` with SVID token returns 403 `FORBIDDEN`
- `delete_secret` with SVID token returns 403 `FORBIDDEN`
- `get_secret` succeeds with service token (skips ABAC)
- `get_secret` succeeds with valid SVID (ABAC checked)
- `list_secrets` succeeds with service token (all in scope)
- `list_secrets` succeeds with valid SVID (own scope only)
- `get_audit_logs` with SVID token returns 403 `FORBIDDEN`
- `get_audit_logs` succeeds with service token

**DEK rotation:**

- `rotate_dek` with SVID token returns 403 `FORBIDDEN`
- `rotate_dek` with service token succeeds (200, returns new version)
- `rotate_dek` for non-existent secret returns 404 `SECRET_NOT_FOUND`
- Secret value is still readable after DEK rotation (plaintext unchanged)
- DEK rotation increments the secret version
- `dek_rotated` audit event is logged after rotation

### Future Work (Phase 2)

The following items are deferred to Phase 2:

- **Master key versioning**: Support multiple KEK versions for gradual master key rotation
- **Request logging/metrics**: HTTP request metrics middleware (method, path, status, duration)
- **Rate limiting**: See moto-throttle.md spec
