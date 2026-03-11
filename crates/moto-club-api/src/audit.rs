//! Audit log query endpoint for moto-club.
//!
//! Provides `GET /api/v1/audit/logs` for querying audit events
//! with filters for `event_type`, `principal_id`, `resource_type`,
//! time range, and pagination.
//!
//! Supports fan-out to keybox's `/audit/logs` endpoint when the `service`
//! filter is not set or is `keybox`. Results are merged by timestamp
//! (newest first). If keybox is unreachable, moto-club returns only its
//! own events with a warning.

use std::time::Duration;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use moto_club_db::audit_repo;

use crate::{ApiError, AppState, error_codes};

/// Timeout for keybox audit log fan-out requests.
const KEYBOX_AUDIT_TIMEOUT: Duration = Duration::from_secs(5);

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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLogsResponse {
    /// Matching audit events.
    pub events: Vec<AuditEventResponse>,
    /// Total number of matching events (before pagination).
    pub total: i64,
    /// Maximum results returned.
    pub limit: i64,
    /// Pagination offset.
    pub offset: i64,
    /// Warnings (e.g. "keybox unavailable"). Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Keybox audit logs response (for deserialization of fan-out response).
#[derive(Debug, Deserialize)]
struct KeyboxAuditResponse {
    entries: Vec<KeyboxAuditEntry>,
    total: usize,
}

/// A single audit entry from keybox's `/audit/logs` response.
#[derive(Debug, Deserialize)]
struct KeyboxAuditEntry {
    id: String,
    event_type: String,
    service: String,
    principal_type: String,
    principal_id: String,
    action: String,
    resource_type: String,
    resource_id: String,
    outcome: String,
    metadata: serde_json::Value,
    #[serde(default)]
    client_ip: Option<String>,
    timestamp: String,
}

/// GET /api/v1/audit/logs
///
/// Query audit log entries with optional filters. Service token auth required.
/// When `service` is omitted or `keybox`, fans out to keybox's `/audit/logs`
/// endpoint in parallel and merges results by timestamp (newest first).
async fn get_audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditLogsQuery>,
) -> Result<(StatusCode, Json<AuditLogsResponse>), (StatusCode, Json<ApiError>)> {
    validate_service_token(&headers, state.service_token.as_ref())?;

    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0).max(0);

    let service_filter = query.service.as_deref();
    let query_moto_club = service_filter.is_none() || service_filter == Some("moto-club");
    let query_keybox = service_filter.is_none() || service_filter == Some("keybox");

    // ai-proxy events are log-only in v1, not queryable
    if service_filter == Some("ai-proxy") {
        return Ok((
            StatusCode::OK,
            Json(AuditLogsResponse {
                events: vec![],
                total: 0,
                limit,
                offset,
                warnings: vec![
                    "ai-proxy audit events are log-only and not queryable via the API in v1"
                        .to_string(),
                ],
            }),
        ));
    }

    let mut warnings = Vec::new();

    // Query local audit log and keybox in parallel
    // Each service is queried with offset+limit rows (offset is NOT forwarded).
    // Results are merged, sorted by timestamp, then offset is applied to the merged set.
    let fetch_limit = offset + limit;
    let (local_result, keybox_result) = tokio::join!(
        query_local_audit(&state, &query, query_moto_club, fetch_limit, 0),
        async {
            if query_keybox {
                query_keybox_fanout(&state, &query, fetch_limit).await
            } else {
                Ok(None)
            }
        }
    );

    let (mut events, mut total) = local_result?;

    match keybox_result {
        Ok(Some((keybox_events, keybox_total))) => {
            total += keybox_total;
            events.extend(keybox_events);
        }
        Ok(None) => {}
        Err(warning) => warnings.push(warning),
    }

    // Sort merged results by timestamp (newest first), apply offset, then truncate to limit
    events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let offset_usize = offset as usize;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let limit_usize = limit as usize;

    // Skip offset rows, then take up to limit rows
    if offset_usize < events.len() {
        events.drain(0..offset_usize);
        events.truncate(limit_usize);
    } else {
        events.clear();
    }

    Ok((
        StatusCode::OK,
        Json(AuditLogsResponse {
            events,
            total,
            limit,
            offset,
            warnings,
        }),
    ))
}

/// Queries moto-club's own `audit_log` table.
async fn query_local_audit(
    state: &AppState,
    query: &AuditLogsQuery,
    enabled: bool,
    limit: i64,
    offset: i64,
) -> Result<(Vec<AuditEventResponse>, i64), (StatusCode, Json<ApiError>)> {
    if !enabled {
        return Ok((vec![], 0));
    }

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
            tracing::error!(error = %e, "failed to query moto-club audit logs");
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

    Ok((events, result.total))
}

/// Queries keybox via fan-out if URL and token are configured.
async fn query_keybox_fanout(
    state: &AppState,
    query: &AuditLogsQuery,
    limit: i64,
) -> Result<Option<(Vec<AuditEventResponse>, i64)>, String> {
    let (Some(keybox_url), Some(keybox_token)) = (&state.keybox_url, &state.keybox_service_token)
    else {
        return Ok(None);
    };

    query_keybox_audit(keybox_url, keybox_token, query, limit).await
}

/// Queries keybox's `/audit/logs` endpoint with the same filter parameters.
///
/// Returns `Ok(Some((events, total)))` on success, `Ok(None)` if not applicable,
/// or `Err(warning_message)` if keybox is unreachable.
async fn query_keybox_audit(
    keybox_url: &str,
    keybox_token: &str,
    query: &AuditLogsQuery,
    limit: i64,
) -> Result<Option<(Vec<AuditEventResponse>, i64)>, String> {
    let client = reqwest::Client::builder()
        .timeout(KEYBOX_AUDIT_TIMEOUT)
        .build()
        .map_err(|e| {
            tracing::warn!(error = %e, "failed to create HTTP client for keybox audit fan-out");
            "keybox unavailable".to_string()
        })?;

    let url = format!("{}/audit/logs", keybox_url.trim_end_matches('/'));
    let mut request = client
        .get(&url)
        .header("Authorization", format!("Bearer {keybox_token}"));

    // Forward filter parameters to keybox
    if let Some(ref event_type) = query.event_type {
        request = request.query(&[("event_type", event_type.as_str())]);
    }
    if let Some(ref principal_id) = query.principal_id {
        request = request.query(&[("principal_id", principal_id.as_str())]);
    }
    if let Some(ref resource_type) = query.resource_type {
        request = request.query(&[("resource_type", resource_type.as_str())]);
    }
    if let Some(ref since) = query.since {
        request = request.query(&[("since", &since.to_rfc3339())]);
    }
    if let Some(ref until) = query.until {
        request = request.query(&[("until", &until.to_rfc3339())]);
    }
    request = request.query(&[("limit", &limit.to_string())]);

    let response = request.send().await.map_err(|e| {
        tracing::warn!(error = %e, "keybox audit fan-out request failed");
        "keybox unavailable".to_string()
    })?;

    if !response.status().is_success() {
        tracing::warn!(
            status = %response.status(),
            "keybox audit fan-out returned non-success status"
        );
        return Err("keybox unavailable".to_string());
    }

    let body: KeyboxAuditResponse = response.json().await.map_err(|e| {
        tracing::warn!(error = %e, "failed to parse keybox audit response");
        "keybox unavailable".to_string()
    })?;

    let original_count = body.entries.len();
    let events: Vec<AuditEventResponse> = body
        .entries
        .into_iter()
        .filter_map(|e| {
            let timestamp = match chrono::DateTime::parse_from_rfc3339(&e.timestamp) {
                Ok(ts) => ts.with_timezone(&Utc),
                Err(parse_err) => {
                    tracing::warn!(
                        event_id = %e.id,
                        timestamp = %e.timestamp,
                        error = %parse_err,
                        "keybox audit event has invalid timestamp, skipping"
                    );
                    return None;
                }
            };
            Some(AuditEventResponse {
                id: e.id,
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
                timestamp,
            })
        })
        .collect();

    let parsed_count = events.len();
    if parsed_count < original_count {
        tracing::warn!(
            original = original_count,
            parsed = parsed_count,
            dropped = original_count - parsed_count,
            "some keybox audit events were dropped due to timestamp parse errors"
        );
    }

    // Adjust total to reflect actual parsed events, not the original count.
    // If keybox returned N events but M failed to parse, we must reduce the
    // total by M to maintain pagination correctness.
    #[allow(clippy::cast_possible_wrap)]
    let dropped = (original_count - parsed_count) as i64;
    #[allow(clippy::cast_possible_wrap)]
    let adjusted_total = (body.total as i64).saturating_sub(dropped);

    Ok(Some((events, adjusted_total)))
}

/// Validates the service token from the Authorization header.
///
/// Uses constant-time comparison to prevent timing attacks.
/// Returns `Ok(())` if the token matches the configured service token.
pub(crate) fn validate_service_token(
    headers: &HeaderMap,
    expected_token: Option<&String>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    use subtle::ConstantTimeEq;

    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    error_codes::UNAUTHORIZED,
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
                    error_codes::UNAUTHORIZED,
                    "Invalid Authorization header format, expected 'Bearer <token>'",
                )),
            )
        })?;

    let expected = expected_token.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::SERVICE_TOKEN_NOT_CONFIGURED,
                "Service token not configured on server",
            )),
        )
    })?;

    if token.as_bytes().ct_eq(expected.as_bytes()).into() {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::FORBIDDEN,
                "Invalid service token",
            )),
        ))
    }
}

/// Router for audit log endpoints.
pub fn router() -> Router<AppState> {
    Router::new().route("/api/v1/audit/logs", get(get_audit_logs))
}

#[cfg(test)]
#[path = "audit_test.rs"]
mod tests;
