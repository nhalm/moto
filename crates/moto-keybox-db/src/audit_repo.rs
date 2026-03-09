//! Audit log repository for `PostgreSQL`.
//!
//! Provides operations for inserting and querying audit log entries
//! using the unified audit schema.

use crate::{AuditEventType, AuditLogEntry, DbPool, DbResult, PrincipalType};

/// Parameters for inserting an audit log entry.
#[derive(Debug)]
pub struct InsertAuditEntry<'a> {
    /// The type of event.
    pub event_type: AuditEventType,
    /// Which service produced the event.
    pub service: &'a str,
    /// Principal type (garage, bike, service).
    pub principal_type: PrincipalType,
    /// SPIFFE ID or service name.
    pub principal_id: &'a str,
    /// What happened (create, read, delete, `auth_fail`, etc.).
    pub action: &'a str,
    /// What was acted on (secret, svid, token, etc.).
    pub resource_type: &'a str,
    /// Identifier of the resource.
    pub resource_id: &'a str,
    /// Result: success, denied, or error.
    pub outcome: &'a str,
    /// Service-specific additional context (no sensitive data).
    pub metadata: serde_json::Value,
    /// Source IP from request headers or socket addr.
    pub client_ip: Option<&'a str>,
}

/// Inserts an audit log entry into the database.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub async fn insert_audit_entry(
    pool: &DbPool,
    entry: &InsertAuditEntry<'_>,
) -> DbResult<AuditLogEntry> {
    let row = sqlx::query_as::<_, AuditLogEntry>(
        r"
        INSERT INTO audit_log (event_type, service, principal_type, principal_id, action, resource_type, resource_id, outcome, metadata, client_ip)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING *
        ",
    )
    .bind(entry.event_type)
    .bind(entry.service)
    .bind(entry.principal_type)
    .bind(entry.principal_id)
    .bind(entry.action)
    .bind(entry.resource_type)
    .bind(entry.resource_id)
    .bind(entry.outcome)
    .bind(&entry.metadata)
    .bind(entry.client_ip)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Query parameters for listing audit logs.
#[derive(Debug, Default)]
pub struct AuditLogQuery {
    /// Filter by event type.
    pub event_type: Option<AuditEventType>,
    /// Filter by principal ID.
    pub principal_id: Option<String>,
    /// Filter by resource type.
    pub resource_type: Option<String>,
    /// Filter by resource ID.
    pub resource_id: Option<String>,
    /// Maximum number of entries to return.
    pub limit: Option<i64>,
    /// Number of entries to skip.
    pub offset: Option<i64>,
}

/// Lists audit log entries with optional filters.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_audit_entries(
    pool: &DbPool,
    query: &AuditLogQuery,
) -> DbResult<Vec<AuditLogEntry>> {
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);

    let rows = sqlx::query_as::<_, AuditLogEntry>(
        r"
        SELECT * FROM audit_log
        WHERE ($1::text IS NULL OR event_type = $1)
          AND ($2::text IS NULL OR principal_id = $2)
          AND ($3::text IS NULL OR resource_type = $3)
          AND ($4::text IS NULL OR resource_id = $4)
        ORDER BY timestamp DESC
        LIMIT $5 OFFSET $6
        ",
    )
    .bind(query.event_type.map(|e| e.to_string()))
    .bind(query.principal_id.as_deref())
    .bind(query.resource_type.as_deref())
    .bind(query.resource_id.as_deref())
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Counts total audit log entries matching the filters.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn count_audit_entries(pool: &DbPool, query: &AuditLogQuery) -> DbResult<i64> {
    let row: (i64,) = sqlx::query_as(
        r"
        SELECT COUNT(*) FROM audit_log
        WHERE ($1::text IS NULL OR event_type = $1)
          AND ($2::text IS NULL OR principal_id = $2)
          AND ($3::text IS NULL OR resource_type = $3)
          AND ($4::text IS NULL OR resource_id = $4)
        ",
    )
    .bind(query.event_type.map(|e| e.to_string()))
    .bind(query.principal_id.as_deref())
    .bind(query.resource_type.as_deref())
    .bind(query.resource_id.as_deref())
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

#[cfg(test)]
#[path = "audit_repo_test.rs"]
mod audit_repo_test;
