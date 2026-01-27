-- Initial keybox database schema
-- Creates tables for secrets management with envelope encryption

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

-- Encrypted DEKs (must be created before secret_versions due to FK)
CREATE TABLE encrypted_deks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    encrypted_key BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
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

-- Indexes for common queries
CREATE INDEX idx_secrets_scope ON secrets(scope);
CREATE INDEX idx_secrets_service ON secrets(service) WHERE service IS NOT NULL;
CREATE INDEX idx_audit_log_timestamp ON audit_log(timestamp);
CREATE INDEX idx_audit_log_spiffe_id ON audit_log(spiffe_id);
