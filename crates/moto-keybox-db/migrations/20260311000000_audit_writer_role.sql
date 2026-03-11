-- Tamper-evident audit log: INSERT-only role for application writes
-- See specs/audit-logging.md and specs/compliance.md (CC7 - tamper-evidence)
--
-- This migration implements append-only audit logging:
-- 1. audit_writer role can only INSERT to audit_log (no UPDATE/DELETE)
-- 2. Application user (moto) is granted audit_writer role
-- 3. Retention function uses SECURITY DEFINER to delete expired records

-- Create audit_writer role (if it doesn't exist)
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'audit_writer') THEN
        CREATE ROLE audit_writer;
    END IF;
END
$$;

-- Grant INSERT-only permission on audit_log table
-- Explicitly REVOKE UPDATE and DELETE to prevent tampering
GRANT INSERT ON audit_log TO audit_writer;
REVOKE UPDATE, DELETE ON audit_log FROM audit_writer;

-- Grant audit_writer role to the application user
-- Using CURRENT_USER to support both production (moto) and test (moto_test) environments
GRANT audit_writer TO CURRENT_USER;

-- Create retention function with SECURITY DEFINER for privileged deletion
-- This allows the retention process to delete expired records despite audit_writer restrictions
-- Retention period: 90 days (per specs/audit-logging.md)
CREATE OR REPLACE FUNCTION delete_expired_audit_logs()
RETURNS INTEGER
LANGUAGE plpgsql
SECURITY DEFINER
AS $$
DECLARE
    deleted_count INTEGER;
BEGIN
    DELETE FROM audit_log
    WHERE timestamp < NOW() - INTERVAL '90 days';

    GET DIAGNOSTICS deleted_count = ROW_COUNT;
    RETURN deleted_count;
END;
$$;

-- Grant execute permission on the retention function to the application user
-- This allows the reconciler to call the function
-- Using CURRENT_USER to support both production (moto) and test (moto_test) environments
GRANT EXECUTE ON FUNCTION delete_expired_audit_logs() TO CURRENT_USER;

-- Comment for documentation
COMMENT ON FUNCTION delete_expired_audit_logs() IS
    'Deletes audit log entries older than 90 days. Uses SECURITY DEFINER to bypass audit_writer INSERT-only restrictions.';
