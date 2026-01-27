//! REST API handlers for moto-keybox.
//!
//! This module provides:
//! - Authentication endpoint (`/auth/token`)
//! - Secrets endpoints (`/secrets/*`)
//! - Audit endpoint (`/audit/logs`)
//! - Health endpoint (`/health`)

use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::abac::{PolicyEngine, Resource, Scope};
use crate::crypto::KeyManager;
use crate::db::{self, DbPool};
use crate::svid::{IssueSvidInput, PrincipalType, SvidClaims, SvidIssuer};

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    /// Database connection pool.
    pub db_pool: DbPool,
    /// Key manager for encryption and signing.
    pub key_manager: Arc<KeyManager>,
    /// SVID issuer.
    pub svid_issuer: SvidIssuer,
    /// ABAC policy engine.
    pub policy_engine: PolicyEngine,
    /// Optional service token for moto-club auth.
    pub service_token: Option<String>,
}

impl AppState {
    /// Creates a new `AppState`.
    #[must_use]
    pub fn new(
        db_pool: DbPool,
        key_manager: Arc<KeyManager>,
        svid_ttl: Duration,
        service_token: Option<String>,
    ) -> Self {
        Self {
            db_pool,
            key_manager,
            svid_issuer: SvidIssuer::new(svid_ttl),
            policy_engine: PolicyEngine::new(),
            service_token,
        }
    }
}

/// Creates the main API router with all routes.
pub fn router(state: AppState) -> Router {
    Router::new()
        // Health
        .route("/health", get(health))
        // Auth
        .route("/auth/token", post(issue_token))
        // Secrets
        .route("/secrets/{scope}/{name}", get(get_secret))
        .route("/secrets/{scope}/{name}", post(set_secret))
        .route("/secrets/{scope}/{name}", delete(delete_secret))
        .route("/secrets/{scope}", get(list_secrets))
        // Audit
        .route("/audit/logs", get(get_audit_logs))
        .with_state(state)
}

// ============================================================================
// API Types
// ============================================================================

/// API error response.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    /// Error details.
    pub error: ApiErrorDetail,
}

/// API error detail.
#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorDetail {
    /// Error code.
    pub code: String,
    /// Human-readable message.
    pub message: String,
}

impl ApiError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

/// Error codes.
pub mod error_codes {
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
    pub const FORBIDDEN: &str = "FORBIDDEN";
    pub const SECRET_NOT_FOUND: &str = "SECRET_NOT_FOUND";
    pub const SECRET_ALREADY_EXISTS: &str = "SECRET_ALREADY_EXISTS";
    pub const INVALID_SCOPE: &str = "INVALID_SCOPE";
    pub const SVID_EXPIRED: &str = "SVID_EXPIRED";
    pub const SVID_INVALID: &str = "SVID_INVALID";
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
}

// ============================================================================
// Health Endpoint
// ============================================================================

/// Health check response.
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "moto-keybox",
    })
}

// ============================================================================
// Auth Endpoints
// ============================================================================

/// Request to issue an SVID token.
#[derive(Debug, Deserialize)]
struct IssueTokenRequest {
    /// Principal type.
    principal_type: String,
    /// Principal ID.
    principal_id: String,
    /// Pod UID (optional).
    pod_uid: Option<String>,
    /// Pod namespace (optional).
    pod_namespace: Option<String>,
    /// Pod name (optional).
    pod_name: Option<String>,
    /// Associated service (optional).
    service: Option<String>,
}

/// Response containing the issued SVID.
#[derive(Debug, Serialize)]
struct IssueTokenResponse {
    /// The signed SVID JWT.
    token: String,
    /// Expiration timestamp (Unix).
    expires_at: i64,
}

/// Issue an SVID token.
///
/// In production, this validates a K8s ServiceAccount JWT.
/// For MVP, it accepts service token auth from moto-club.
async fn issue_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IssueTokenRequest>,
) -> Result<Json<IssueTokenResponse>, (StatusCode, Json<ApiError>)> {
    // Validate authorization (service token for MVP)
    validate_service_auth(&state, &headers)?;

    // Parse principal type
    let principal_type = match req.principal_type.as_str() {
        "garage" => PrincipalType::Garage,
        "bike" => PrincipalType::Bike,
        "service" => PrincipalType::Service,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    "INVALID_PRINCIPAL_TYPE",
                    "invalid principal type",
                )),
            ));
        }
    };

    let input = IssueSvidInput {
        principal_type,
        principal_id: req.principal_id.clone(),
        pod_uid: req.pod_uid,
        pod_namespace: req.pod_namespace,
        pod_name: req.pod_name,
        service: req.service,
    };

    let token = state
        .svid_issuer
        .issue(state.key_manager.signing_key(), input)
        .map_err(|e| {
            error!(error = %e, "failed to issue SVID");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "failed to issue token",
                )),
            )
        })?;

    // Calculate expiration
    #[allow(clippy::cast_possible_wrap)]
    let expires_at = chrono::Utc::now().timestamp() + 900; // Default 15 min

    info!(
        principal_type = %req.principal_type,
        principal_id = %req.principal_id,
        "issued SVID"
    );

    Ok(Json(IssueTokenResponse { token, expires_at }))
}

// ============================================================================
// Secrets Endpoints
// ============================================================================

/// Path parameters for secret operations.
#[derive(Debug, Deserialize)]
struct SecretPath {
    scope: String,
    name: String,
}

/// Query parameters for secret operations.
#[derive(Debug, Deserialize)]
struct SecretQuery {
    /// Service name (for service/instance scope).
    service: Option<String>,
    /// Instance ID (for instance scope).
    instance_id: Option<String>,
}

/// Request to set a secret.
#[derive(Debug, Deserialize)]
struct SetSecretRequest {
    /// The secret value.
    value: String,
    /// Service name (for service/instance scope).
    service: Option<String>,
    /// Instance ID (for instance scope).
    instance_id: Option<String>,
}

/// Response from getting a secret.
#[derive(Debug, Serialize)]
struct GetSecretResponse {
    /// The secret value.
    value: String,
    /// Version number.
    version: i32,
}

/// Response from setting a secret.
#[derive(Debug, Serialize)]
struct SetSecretResponse {
    /// Whether the secret was created (vs updated).
    created: bool,
    /// Version number.
    version: i32,
}

/// Response from listing secrets.
#[derive(Debug, Serialize)]
struct ListSecretsResponse {
    /// Secret names.
    secrets: Vec<SecretInfo>,
}

/// Secret info (no value).
#[derive(Debug, Serialize)]
struct SecretInfo {
    name: String,
    version: i32,
    updated_at: chrono::DateTime<chrono::Utc>,
}

/// Get a secret value.
async fn get_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(path): Path<SecretPath>,
    Query(query): Query<SecretQuery>,
) -> Result<Json<GetSecretResponse>, (StatusCode, Json<ApiError>)> {
    // Validate SVID
    let claims = validate_svid(&state, &headers)?;

    // Parse scope
    let scope = Scope::from_str(&path.scope).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(error_codes::INVALID_SCOPE, "invalid scope")),
        )
    })?;

    // Check ABAC policy
    let resource = Resource {
        scope,
        service: query.service.as_deref(),
        instance_id: query.instance_id.as_deref(),
        name: &path.name,
    };

    let decision = state.policy_engine.evaluate(&claims, &resource);
    if !decision.is_allowed() {
        warn!(
            principal = %claims.sub,
            secret = %path.name,
            scope = %path.scope,
            "access denied"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(error_codes::FORBIDDEN, "access denied")),
        ));
    }

    // Get secret from database
    let secret = db::get_secret(
        &state.db_pool,
        &path.scope,
        query.service.as_deref(),
        query.instance_id.as_deref(),
        &path.name,
    )
    .await
    .map_err(|e| match e {
        db::DbError::NotFound { .. } => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                error_codes::SECRET_NOT_FOUND,
                "secret not found",
            )),
        ),
        _ => {
            error!(error = %e, "database error getting secret");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
            )
        }
    })?;

    // Get secret version with ciphertext
    let version = db::get_secret_version(&state.db_pool, secret.id)
        .await
        .map_err(|e| {
            error!(error = %e, "database error getting secret version");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
            )
        })?;

    // Get and decrypt DEK
    let encrypted_dek = db::get_dek(&state.db_pool, version.dek_id)
        .await
        .map_err(|e| {
            error!(error = %e, "database error getting DEK");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
            )
        })?;

    let dek = state
        .key_manager
        .decrypt_dek(&encrypted_dek.encrypted_key, &encrypted_dek.nonce)
        .map_err(|e| {
            error!(error = %e, "failed to decrypt DEK");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "decryption error",
                )),
            )
        })?;

    // Decrypt secret value
    let plaintext = state
        .key_manager
        .decrypt_secret(&version.ciphertext, &version.nonce, &dek)
        .map_err(|e| {
            error!(error = %e, "failed to decrypt secret");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "decryption error",
                )),
            )
        })?;

    let value = String::from_utf8(plaintext).map_err(|_| {
        error!("secret value is not valid UTF-8");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::INTERNAL_ERROR,
                "invalid secret encoding",
            )),
        )
    })?;

    // Audit log
    let _ = db::create_audit_entry(
        &state.db_pool,
        db::CreateAuditEntry {
            event_type: "accessed".to_string(),
            principal_type: Some(claims.principal_type.to_string()),
            principal_id: Some(claims.principal_id.clone()),
            spiffe_id: Some(claims.sub.clone()),
            secret_scope: Some(path.scope.clone()),
            secret_name: Some(path.name.clone()),
        },
    )
    .await;

    info!(
        principal = %claims.sub,
        secret = %path.name,
        scope = %path.scope,
        "secret accessed"
    );

    Ok(Json(GetSecretResponse {
        value,
        version: secret.current_version,
    }))
}

/// Set a secret value.
async fn set_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(path): Path<SecretPath>,
    Json(req): Json<SetSecretRequest>,
) -> Result<Json<SetSecretResponse>, (StatusCode, Json<ApiError>)> {
    // Validate service auth (only moto-club can set secrets for MVP)
    validate_service_auth(&state, &headers)?;

    // Parse scope
    let scope = Scope::from_str(&path.scope).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(error_codes::INVALID_SCOPE, "invalid scope")),
        )
    })?;

    // Generate new DEK and encrypt it
    let (encrypted_dek, dek_nonce, dek) = state.key_manager.generate_dek().map_err(|e| {
        error!(error = %e, "failed to generate DEK");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::INTERNAL_ERROR,
                "encryption error",
            )),
        )
    })?;

    // Save DEK to database
    let dek_record = db::create_dek(&state.db_pool, encrypted_dek, dek_nonce)
        .await
        .map_err(|e| {
            error!(error = %e, "database error creating DEK");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
            )
        })?;

    // Encrypt the secret value
    let (ciphertext, nonce) = state
        .key_manager
        .encrypt_secret(req.value.as_bytes(), &dek)
        .map_err(|e| {
            error!(error = %e, "failed to encrypt secret");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "encryption error",
                )),
            )
        })?;

    // Check if secret exists
    let existing = db::get_secret(
        &state.db_pool,
        scope.as_str(),
        req.service.as_deref(),
        req.instance_id.as_deref(),
        &path.name,
    )
    .await;

    let (created, version) = match existing {
        Ok(secret) => {
            // Update existing secret
            let updated =
                db::update_secret(&state.db_pool, secret.id, ciphertext, nonce, dek_record.id)
                    .await
                    .map_err(|e| {
                        error!(error = %e, "database error updating secret");
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
                        )
                    })?;
            (false, updated.current_version)
        }
        Err(db::DbError::NotFound { .. }) => {
            // Create new secret
            let created = db::create_secret(
                &state.db_pool,
                db::CreateSecret {
                    scope: scope.as_str().to_string(),
                    service: req.service.clone(),
                    instance_id: req.instance_id.clone(),
                    name: path.name.clone(),
                },
                ciphertext,
                nonce,
                dek_record.id,
            )
            .await
            .map_err(|e| {
                error!(error = %e, "database error creating secret");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
                )
            })?;
            (true, created.current_version)
        }
        Err(e) => {
            error!(error = %e, "database error checking secret");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
            ));
        }
    };

    // Audit log
    let _ = db::create_audit_entry(
        &state.db_pool,
        db::CreateAuditEntry {
            event_type: if created { "created" } else { "updated" }.to_string(),
            principal_type: Some("service".to_string()),
            principal_id: Some("moto-club".to_string()),
            spiffe_id: None,
            secret_scope: Some(path.scope.clone()),
            secret_name: Some(path.name.clone()),
        },
    )
    .await;

    info!(
        secret = %path.name,
        scope = %path.scope,
        created,
        version,
        "secret set"
    );

    Ok(Json(SetSecretResponse { created, version }))
}

/// Delete a secret.
async fn delete_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(path): Path<SecretPath>,
    Query(query): Query<SecretQuery>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    // Validate service auth (only moto-club can delete secrets for MVP)
    validate_service_auth(&state, &headers)?;

    // Get secret to delete
    let secret = db::get_secret(
        &state.db_pool,
        &path.scope,
        query.service.as_deref(),
        query.instance_id.as_deref(),
        &path.name,
    )
    .await
    .map_err(|e| match e {
        db::DbError::NotFound { .. } => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                error_codes::SECRET_NOT_FOUND,
                "secret not found",
            )),
        ),
        _ => {
            error!(error = %e, "database error getting secret");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
            )
        }
    })?;

    // Soft delete
    db::delete_secret(&state.db_pool, secret.id)
        .await
        .map_err(|e| {
            error!(error = %e, "database error deleting secret");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
            )
        })?;

    // Audit log
    let _ = db::create_audit_entry(
        &state.db_pool,
        db::CreateAuditEntry {
            event_type: "deleted".to_string(),
            principal_type: Some("service".to_string()),
            principal_id: Some("moto-club".to_string()),
            spiffe_id: None,
            secret_scope: Some(path.scope.clone()),
            secret_name: Some(path.name.clone()),
        },
    )
    .await;

    info!(secret = %path.name, scope = %path.scope, "secret deleted");

    Ok(StatusCode::NO_CONTENT)
}

/// List secrets in a scope.
async fn list_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope): Path<String>,
    Query(query): Query<SecretQuery>,
) -> Result<Json<ListSecretsResponse>, (StatusCode, Json<ApiError>)> {
    // Validate service auth for listing
    validate_service_auth(&state, &headers)?;

    // Validate scope
    if Scope::from_str(&scope).is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(error_codes::INVALID_SCOPE, "invalid scope")),
        ));
    }

    let secrets = db::list_secrets(
        &state.db_pool,
        &scope,
        query.service.as_deref(),
        query.instance_id.as_deref(),
    )
    .await
    .map_err(|e| {
        error!(error = %e, "database error listing secrets");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
        )
    })?;

    Ok(Json(ListSecretsResponse {
        secrets: secrets
            .into_iter()
            .map(|s| SecretInfo {
                name: s.name,
                version: s.current_version,
                updated_at: s.updated_at,
            })
            .collect(),
    }))
}

// ============================================================================
// Audit Endpoint
// ============================================================================

/// Query parameters for audit logs.
#[derive(Debug, Deserialize)]
struct AuditQuery {
    /// Filter by SPIFFE ID.
    spiffe_id: Option<String>,
    /// Filter by secret name.
    secret_name: Option<String>,
    /// Limit results.
    #[serde(default = "default_audit_limit")]
    limit: i64,
}

fn default_audit_limit() -> i64 {
    100
}

/// Response from getting audit logs.
#[derive(Debug, Serialize)]
struct AuditLogsResponse {
    entries: Vec<db::AuditLogEntry>,
}

/// Get audit logs.
async fn get_audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditLogsResponse>, (StatusCode, Json<ApiError>)> {
    // Validate service auth for audit access
    validate_service_auth(&state, &headers)?;

    let entries = db::query_audit_logs(
        &state.db_pool,
        query.spiffe_id.as_deref(),
        query.secret_name.as_deref(),
        query.limit.min(1000),
    )
    .await
    .map_err(|e| {
        error!(error = %e, "database error querying audit logs");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(error_codes::INTERNAL_ERROR, "database error")),
        )
    })?;

    Ok(Json(AuditLogsResponse { entries }))
}

// ============================================================================
// Auth Helpers
// ============================================================================

/// Extract and validate SVID from Authorization header.
fn validate_svid(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<SvidClaims, (StatusCode, Json<ApiError>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    error_codes::UNAUTHORIZED,
                    "missing authorization header",
                )),
            )
        })?;

    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::UNAUTHORIZED,
                "invalid authorization header",
            )),
        )
    })?;

    state
        .svid_issuer
        .validate(state.key_manager.verifying_key(), token)
        .map_err(|e| {
            let (code, msg) = match e {
                crate::svid::SvidError::Expired => (error_codes::SVID_EXPIRED, "SVID has expired"),
                _ => (error_codes::SVID_INVALID, "invalid SVID"),
            };
            (StatusCode::UNAUTHORIZED, Json(ApiError::new(code, msg)))
        })
}

/// Validate service token authentication (for moto-club).
fn validate_service_auth(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    // If no service token configured, allow all (dev mode)
    let Some(expected_token) = &state.service_token else {
        return Ok(());
    };

    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    error_codes::UNAUTHORIZED,
                    "missing authorization header",
                )),
            )
        })?;

    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::UNAUTHORIZED,
                "invalid authorization header",
            )),
        )
    })?;

    if token != expected_token {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::UNAUTHORIZED,
                "invalid service token",
            )),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_serialization() {
        let error = ApiError::new("SECRET_NOT_FOUND", "secret not found");
        let json = serde_json::to_string(&error).unwrap();

        assert!(json.contains(r#""code":"SECRET_NOT_FOUND""#));
        assert!(json.contains(r#""message":"secret not found""#));
    }
}
