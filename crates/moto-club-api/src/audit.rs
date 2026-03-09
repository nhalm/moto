//! Audit log query endpoint for moto-club.
//!
//! Provides `GET /api/v1/audit/logs` for querying audit events
//! with filters for `event_type`, `principal_id`, `resource_type`,
//! time range, and pagination.

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use moto_club_db::audit_repo;

use crate::{ApiError, AppState, error_codes};

/// Query parameters for the audit logs endpoint.
#[derive(Debug, Default, Deserialize)]
pub struct AuditLogsQuery {
    /// Filter by service (`keybox`, `moto-club`). If omitted, queries all.
    pub service: Option<String>,
    /// Filter by event type (e.g. `garage_created`, `auth_failed`).
    pub event_type: Option<String>,
    /// Filter by principal ID.
    pub principal_id: Option<String>,
    /// Filter by resource type (e.g. `garage`, `request`).
    pub resource_type: Option<String>,
    /// Events after this timestamp (ISO 8601).
    pub since: Option<DateTime<Utc>>,
    /// Events before this timestamp (ISO 8601).
    pub until: Option<DateTime<Utc>>,
    /// Max results (default 100, max 1000).
    pub limit: Option<i64>,
    /// Pagination offset (default 0).
    pub offset: Option<i64>,
}

/// Audit log event in the API response.
#[derive(Debug, Serialize)]
pub struct AuditEventResponse {
    /// Unique event ID.
    pub id: String,
    /// Event category.
    pub event_type: String,
    /// Service that produced the event.
    pub service: String,
    /// Principal type (garage, bike, service, anonymous).
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
}

/// Response for the audit logs endpoint.
#[derive(Debug, Serialize)]
pub struct AuditLogsResponse {
    /// Matching audit events.
    pub events: Vec<AuditEventResponse>,
    /// Total number of matching events (before pagination).
    pub total: i64,
    /// Maximum results returned.
    pub limit: i64,
    /// Pagination offset.
    pub offset: i64,
}

/// GET /api/v1/audit/logs
///
/// Query audit log entries with optional filters. Service token auth required.
async fn get_audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditLogsQuery>,
) -> Result<(StatusCode, Json<AuditLogsResponse>), (StatusCode, Json<ApiError>)> {
    // Auth: require a bearer token (service token enforcement is a separate work item)
    let _token = extract_bearer_token(&headers)?;

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0).max(0);

    let db_query = audit_repo::AuditLogQuery {
        service: query.service.as_deref(),
        event_type: query.event_type.as_deref(),
        principal_id: query.principal_id.as_deref(),
        resource_type: query.resource_type.as_deref(),
        since: query.since,
        until: query.until,
        limit,
        offset,
    };

    let result = audit_repo::query(&state.db_pool, &db_query)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to query audit logs");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::DATABASE_ERROR,
                    "Failed to query audit logs",
                )),
            )
        })?;

    let events = result
        .events
        .into_iter()
        .map(|e| AuditEventResponse {
            id: e.id.to_string(),
            event_type: e.event_type,
            service: e.service,
            principal_type: e.principal_type,
            principal_id: e.principal_id,
            action: e.action,
            resource_type: e.resource_type,
            resource_id: e.resource_id,
            outcome: e.outcome,
            metadata: e.metadata,
            client_ip: e.client_ip,
            timestamp: e.timestamp,
        })
        .collect();

    Ok((
        StatusCode::OK,
        Json(AuditLogsResponse {
            events,
            total: result.total,
            limit,
            offset,
        }),
    ))
}

/// Extract bearer token from Authorization header.
fn extract_bearer_token(headers: &HeaderMap) -> Result<String, (StatusCode, Json<ApiError>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    "UNAUTHORIZED",
                    "Missing Authorization header",
                )),
            )
        })?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    "UNAUTHORIZED",
                    "Invalid Authorization header format, expected 'Bearer <token>'",
                )),
            )
        })?;

    if token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new("UNAUTHORIZED", "Empty Bearer token")),
        ));
    }

    Ok(token.to_string())
}

/// Router for audit log endpoints.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/audit/logs", get(get_audit_logs))
}
