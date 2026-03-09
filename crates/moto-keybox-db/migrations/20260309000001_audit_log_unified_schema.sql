-- Migrate audit_log table to unified audit schema.
-- Adds: service, action, resource_type, resource_id, outcome, metadata, client_ip
-- Maps: spiffe_id/secret_scope/secret_name → resource_type/resource_id
-- Updates: event_type values to unified naming (accessed → secret_accessed, etc.)

-- Add new columns with defaults for existing rows
ALTER TABLE audit_log ADD COLUMN service TEXT NOT NULL DEFAULT 'keybox';
ALTER TABLE audit_log ADD COLUMN action TEXT NOT NULL DEFAULT '';
ALTER TABLE audit_log ADD COLUMN resource_type TEXT NOT NULL DEFAULT '';
ALTER TABLE audit_log ADD COLUMN resource_id TEXT NOT NULL DEFAULT '';
ALTER TABLE audit_log ADD COLUMN outcome TEXT NOT NULL DEFAULT 'success';
ALTER TABLE audit_log ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';
ALTER TABLE audit_log ADD COLUMN client_ip TEXT;

-- Migrate existing data: populate action and resource_type/resource_id from old columns
UPDATE audit_log SET
    action = CASE event_type
        WHEN 'accessed' THEN 'read'
        WHEN 'created' THEN 'create'
        WHEN 'updated' THEN 'update'
        WHEN 'deleted' THEN 'delete'
        WHEN 'dek_rotated' THEN 'rotate'
        WHEN 'svid_issued' THEN 'create'
        WHEN 'auth_failed' THEN 'auth_fail'
        WHEN 'access_denied' THEN 'auth_fail'
        ELSE ''
    END,
    resource_type = CASE
        WHEN event_type IN ('accessed', 'created', 'updated', 'deleted', 'dek_rotated') THEN 'secret'
        WHEN event_type = 'svid_issued' THEN 'svid'
        WHEN event_type IN ('auth_failed', 'access_denied') THEN 'token'
        ELSE ''
    END,
    resource_id = CASE
        WHEN secret_scope IS NOT NULL AND secret_name IS NOT NULL
            THEN secret_scope || '/' || secret_name
        WHEN secret_name IS NOT NULL THEN secret_name
        WHEN spiffe_id IS NOT NULL THEN spiffe_id
        ELSE ''
    END,
    -- Map principal_id from spiffe_id if principal_id is null
    principal_id = COALESCE(principal_id, spiffe_id, ''),
    principal_type = COALESCE(principal_type, 'anonymous');

-- Update event_type values to unified naming
UPDATE audit_log SET event_type = CASE event_type
    WHEN 'accessed' THEN 'secret_accessed'
    WHEN 'created' THEN 'secret_created'
    WHEN 'updated' THEN 'secret_updated'
    WHEN 'deleted' THEN 'secret_deleted'
    ELSE event_type
END;

-- Make principal_type and principal_id NOT NULL (unified schema requirement)
ALTER TABLE audit_log ALTER COLUMN principal_type SET NOT NULL;
ALTER TABLE audit_log ALTER COLUMN principal_id SET NOT NULL;

-- Drop old columns
ALTER TABLE audit_log DROP COLUMN spiffe_id;
ALTER TABLE audit_log DROP COLUMN secret_scope;
ALTER TABLE audit_log DROP COLUMN secret_name;

-- Drop old index on spiffe_id
DROP INDEX IF EXISTS idx_audit_log_spiffe_id;

-- Add unified schema indexes
CREATE INDEX idx_audit_log_principal ON audit_log(principal_id);
CREATE INDEX idx_audit_log_event_type ON audit_log(event_type);
CREATE INDEX idx_audit_log_resource ON audit_log(resource_type, resource_id);
