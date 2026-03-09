//! Audit log repository for moto-club.
//!
//! Provides functions for inserting audit log entries into the
//! `audit_log` table using the unified audit schema.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::{DbPool, DbResult};

/// Audit log entry from the database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct AuditLogEntry {
    /// Unique identifier.
    pub id: uuid::Uuid,
    /// Event category.
    pub event_type: String,
    /// Which service produced the event.
    pub service: String,
    /// Principal type: garage, bike, service, or anonymous.
    pub principal_type: String,
    /// SPIFFE ID or service name.
    pub principal_id: String,
    /// What happened.
    pub action: String,
    /// What was acted on.
    pub resource_type: String,
    /// Identifier of the resource.
    pub resource_id: String,
    /// Result: success, denied, or error.
    pub outcome: String,
    /// Service-specific additional context.
    pub metadata: serde_json::Value,
    /// Source IP from request headers.
    pub client_ip: Option<String>,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
}

/// Parameters for inserting an audit log entry.
#[derive(Debug)]
pub struct InsertAuditEntry<'a> {
    /// Event type (e.g. `garage_created`, `auth_failed`).
    pub event_type: &'a str,
    /// Principal type (e.g. `service`, `garage`).
    pub principal_type: &'a str,
    /// Principal identifier.
    pub principal_id: &'a str,
    /// Action (e.g. `create`, `delete`, `auth_fail`).
    pub action: &'a str,
    /// Resource type (e.g. `garage`, `request`).
    pub resource_type: &'a str,
    /// Resource identifier.
    pub resource_id: &'a str,
    /// Outcome: `success`, `denied`, or `error`.
    pub outcome: &'a str,
    /// Additional metadata (no sensitive data).
    pub metadata: serde_json::Value,
    /// Source IP address.
    pub client_ip: Option<&'a str>,
}

/// Inserts an audit log entry into the database.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub async fn insert(pool: &DbPool, entry: &InsertAuditEntry<'_>) -> DbResult<AuditLogEntry> {
    let row = sqlx::query_as::<_, AuditLogEntry>(
        r"
        INSERT INTO audit_log (event_type, service, principal_type, principal_id, action, resource_type, resource_id, outcome, metadata, client_ip)
        VALUES ($1, 'moto-club', $2, $3, $4, $5, $6, $7, $8, $9)
        RETURNING *
        ",
    )
    .bind(entry.event_type)
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

/// Deletes audit log entries older than the given number of days.
///
/// Deletes at most `batch_size` rows per call to avoid long-running transactions.
/// Returns the number of rows deleted.
///
/// # Errors
///
/// Returns an error if the delete query fails.
pub async fn delete_expired(pool: &DbPool, retention_days: i32, batch_size: i64) -> DbResult<u64> {
    let result = sqlx::query(
        r"
        DELETE FROM audit_log
        WHERE id IN (
            SELECT id FROM audit_log
            WHERE timestamp < now() - make_interval(days => $1)
            ORDER BY timestamp ASC
            LIMIT $2
        )
        ",
    )
    .bind(retention_days)
    .bind(batch_size)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

/// Query parameters for listing audit log entries.
#[derive(Debug, Default)]
pub struct AuditLogQuery<'a> {
    /// Filter by service (e.g. `moto-club`).
    pub service: Option<&'a str>,
    /// Filter by event type.
    pub event_type: Option<&'a str>,
    /// Filter by principal ID.
    pub principal_id: Option<&'a str>,
    /// Filter by resource type.
    pub resource_type: Option<&'a str>,
    /// Events after this timestamp.
    pub since: Option<DateTime<Utc>>,
    /// Events before this timestamp.
    pub until: Option<DateTime<Utc>>,
    /// Maximum number of results (default 100, max 1000).
    pub limit: i64,
    /// Pagination offset.
    pub offset: i64,
}

/// Result of an audit log query including total count.
#[derive(Debug, Serialize)]
pub struct AuditLogQueryResult {
    /// Matching audit log entries.
    pub events: Vec<AuditLogEntry>,
    /// Total number of matching entries (before limit/offset).
    pub total: i64,
}

/// Queries audit log entries with optional filters.
///
/// Returns entries sorted by timestamp descending (newest first).
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn query(pool: &DbPool, q: &AuditLogQuery<'_>) -> DbResult<AuditLogQueryResult> {
    // Build WHERE clauses dynamically
    let mut conditions = Vec::new();
    let mut param_idx = 0u32;

    if q.service.is_some() {
        param_idx += 1;
        conditions.push(format!("service = ${param_idx}"));
    }
    if q.event_type.is_some() {
        param_idx += 1;
        conditions.push(format!("event_type = ${param_idx}"));
    }
    if q.principal_id.is_some() {
        param_idx += 1;
        conditions.push(format!("principal_id = ${param_idx}"));
    }
    if q.resource_type.is_some() {
        param_idx += 1;
        conditions.push(format!("resource_type = ${param_idx}"));
    }
    if q.since.is_some() {
        param_idx += 1;
        conditions.push(format!("timestamp >= ${param_idx}"));
    }
    if q.until.is_some() {
        param_idx += 1;
        conditions.push(format!("timestamp <= ${param_idx}"));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let limit_idx = param_idx + 1;
    let offset_idx = param_idx + 2;

    let count_sql = format!("SELECT COUNT(*) as count FROM audit_log {where_clause}");
    let query_sql = format!(
        "SELECT * FROM audit_log {where_clause} ORDER BY timestamp DESC LIMIT ${limit_idx} OFFSET ${offset_idx}"
    );

    // Build count query
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_sql);
    let mut data_query = sqlx::query_as::<_, AuditLogEntry>(&query_sql);

    // Bind filter params to both queries in the same order
    if let Some(v) = q.service {
        count_query = count_query.bind(v);
        data_query = data_query.bind(v);
    }
    if let Some(v) = q.event_type {
        count_query = count_query.bind(v);
        data_query = data_query.bind(v);
    }
    if let Some(v) = q.principal_id {
        count_query = count_query.bind(v);
        data_query = data_query.bind(v);
    }
    if let Some(v) = q.resource_type {
        count_query = count_query.bind(v);
        data_query = data_query.bind(v);
    }
    if let Some(v) = q.since {
        count_query = count_query.bind(v);
        data_query = data_query.bind(v);
    }
    if let Some(v) = q.until {
        count_query = count_query.bind(v);
        data_query = data_query.bind(v);
    }

    // Bind limit and offset to data query only
    data_query = data_query.bind(q.limit).bind(q.offset);

    let total = count_query.fetch_one(pool).await?;
    let events = data_query.fetch_all(pool).await?;

    Ok(AuditLogQueryResult { events, total })
}

/// Logs an audit event best-effort. Failures are logged as warnings but never block.
pub async fn log_event(pool: &DbPool, entry: InsertAuditEntry<'_>) {
    if let Err(e) = insert(pool, &entry).await {
        tracing::warn!(
            event_type = entry.event_type,
            resource_id = entry.resource_id,
            error = %e,
            "failed to write audit log entry"
        );
    }
}
