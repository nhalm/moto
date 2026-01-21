# Keybox

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

Secrets manager for moto. Provides credentials to garages (wrenching) and bikes (ripping) without baking secrets into containers or code. Uses SPIFFE-inspired identity for authentication and ABAC for authorization.

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

```
1. Garage/Bike pod starts with K8s ServiceAccount JWT

2. Pod → POST /auth/token
   Headers: Authorization: Bearer <K8s SA JWT>

   Keybox:
   - Validates JWT via K8s TokenReview API
   - Fetches pod metadata via K8s API
   - Verifies pod labels (garage-id, bike-id, etc.)
   - Signs SVID JWT with Ed25519 key

   ← Returns: Signed SVID (15 min TTL)

3. Pod caches SVID, refreshes at 14 min (before expiry)
```

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

### API Endpoints

**Authentication:**
```
POST /auth/token
  Request: K8s ServiceAccount JWT
  Response: Signed SVID
```

**Secrets:**
```
GET  /secrets/{scope}/{name}     - Retrieve secret
POST /secrets/{scope}/{name}     - Create/update secret (admin)
DELETE /secrets/{scope}/{name}   - Delete secret (admin)
GET  /secrets/{scope}            - List secrets in scope (names only, no values)
```

**Admin:**
```
GET  /audit/logs                 - Query audit logs
POST /admin/rotate-dek/{name}    - Rotate a secret's DEK
```

### Configuration

```bash
# Required
MOTO_KEYBOX_MASTER_KEY_FILE="/run/secrets/keybox-master-key"
MOTO_KEYBOX_SVID_SIGNING_KEY_FILE="/run/secrets/svid-signing-key"
MOTO_KEYBOX_DATABASE_URL="postgres://keybox:password@localhost:5432/keybox"

# Optional
MOTO_KEYBOX_SVID_TTL_SECONDS="900"  # Default 15 min
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
