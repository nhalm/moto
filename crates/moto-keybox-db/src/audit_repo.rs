//! Audit log repository for `PostgreSQL`.
//!
//! Provides operations for inserting and querying audit log entries.

use crate::{AuditEventType, AuditLogEntry, DbPool, DbResult, PrincipalType, Scope};

/// Inserts an audit log entry into the database.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub async fn insert_audit_entry(
    pool: &DbPool,
    event_type: AuditEventType,
    principal_type: Option<PrincipalType>,
    principal_id: Option<&str>,
    spiffe_id: Option<&str>,
    secret_scope: Option<Scope>,
    secret_name: Option<&str>,
) -> DbResult<AuditLogEntry> {
    let row = sqlx::query_as::<_, AuditLogEntry>(
        r"
        INSERT INTO audit_log (event_type, principal_type, principal_id, spiffe_id, secret_scope, secret_name)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING *
        ",
    )
    .bind(event_type)
    .bind(principal_type)
    .bind(principal_id)
    .bind(spiffe_id)
    .bind(secret_scope)
    .bind(secret_name)
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
    /// Filter by secret name.
    pub secret_name: Option<String>,
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
    // Build the query dynamically based on filters
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);

    let rows = sqlx::query_as::<_, AuditLogEntry>(
        r"
        SELECT * FROM audit_log
        WHERE ($1::text IS NULL OR event_type = $1)
          AND ($2::text IS NULL OR principal_id = $2)
          AND ($3::text IS NULL OR secret_name = $3)
        ORDER BY timestamp DESC
        LIMIT $4 OFFSET $5
        ",
    )
    .bind(query.event_type.map(|e| e.to_string()))
    .bind(query.principal_id.as_deref())
    .bind(query.secret_name.as_deref())
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
          AND ($3::text IS NULL OR secret_name = $3)
        ",
    )
    .bind(query.event_type.map(|e| e.to_string()))
    .bind(query.principal_id.as_deref())
    .bind(query.secret_name.as_deref())
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

#[cfg(test)]
#[path = "audit_repo_test.rs"]
mod audit_repo_test;
