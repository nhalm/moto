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
