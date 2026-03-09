//! REST API endpoints for moto-keybox.
//!
//! Provides HTTP API layer for keybox:
//! - `POST /auth/token` - Exchange K8s `ServiceAccount` JWT for SVID
//! - `GET /secrets/{scope}/{name}` - Retrieve a secret
//! - `POST /secrets/{scope}/{name}` - Create/update a secret
//! - `DELETE /secrets/{scope}/{name}` - Delete a secret
//! - `GET /secrets/{scope}` - List secrets in a scope
//! - `GET /audit/logs` - Query audit logs (admin only)
//!
//! # Authentication
//!
//! All secret endpoints require a valid SVID in the `Authorization: Bearer <svid>` header.
//! The `/auth/token` endpoint exchanges a K8s `ServiceAccount` JWT for an SVID.
//!
//! # Example
//!
//! ```ignore
//! use moto_keybox::api::{AppState, router};
//!
//! let state = AppState::new(master_key, signing_key, validator);
//! let app = router(state);
//!
//! // Run with axum
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//! axum::serve(listener, app).await?;
//! ```

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::Error;
use crate::abac::PolicyEngine;
use crate::envelope::MasterKey;
use crate::repository::SecretRepository;
use crate::svid::{SvidClaims, SvidIssuer, SvidValidator};
use crate::types::{AuditEntry, AuditEventType, PrincipalType, Scope, SecretMetadata, SpiffeId};

/// Default garage SVID TTL in seconds (1 hour per spec).
pub const GARAGE_SVID_TTL_SECS: i64 = 3600;

/// Maximum secret value size in bytes (1 MB per spec).
pub const MAX_SECRET_SIZE_BYTES: usize = 1_048_576;

/// Shared application state for the keybox API.
#[derive(Clone)]
pub struct AppState {
    /// Secret repository (thread-safe).
    pub repository: Arc<RwLock<SecretRepository>>,
    /// SVID issuer for token generation.
    pub svid_issuer: Arc<SvidIssuer>,
    /// SVID validator for token verification.
    pub svid_validator: Arc<SvidValidator>,
    /// Service token for moto-club authentication (static shared token for MVP).
    service_token: Option<String>,
    /// Admin service name for creating synthetic claims on service token auth.
    admin_service: String,
}

impl AppState {
    /// Creates a new `AppState` with the given components.
    #[must_use]
    pub fn new(
        master_key: MasterKey,
        svid_issuer: SvidIssuer,
        svid_validator: SvidValidator,
        admin_service: &str,
    ) -> Self {
        let policy = PolicyEngine::new().with_admin_service(admin_service);
        let repository = SecretRepository::new(master_key, policy);
        Self {
            repository: Arc::new(RwLock::new(repository)),
            svid_issuer: Arc::new(svid_issuer),
            svid_validator: Arc::new(svid_validator),
            service_token: None,
            admin_service: admin_service.to_string(),
        }
    }

    /// Sets the service token for moto-club authentication.
    #[must_use]
    pub fn with_service_token(mut self, token: impl Into<String>) -> Self {
        self.service_token = Some(token.into());
        self
    }

    /// Creates `AppState` from an existing repository.
    #[must_use]
    pub fn with_repository(
        repository: SecretRepository,
        svid_issuer: SvidIssuer,
        svid_validator: SvidValidator,
        admin_service: &str,
    ) -> Self {
        Self {
            repository: Arc::new(RwLock::new(repository)),
            svid_issuer: Arc::new(svid_issuer),
            svid_validator: Arc::new(svid_validator),
            service_token: None,
            admin_service: admin_service.to_string(),
        }
    }

    /// Creates synthetic admin claims for service token callers.
    ///
    /// Used when a service token is validated — bypasses ABAC since the
    /// admin service has full access to all secrets.
    fn service_token_claims(&self) -> SvidClaims {
        SvidClaims::new(
            &SpiffeId::service(&self.admin_service),
            crate::svid::DEFAULT_SVID_TTL_SECS,
        )
    }
}

/// API error response format.
#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    /// Error details.
    pub error: ApiErrorDetail,
}

/// API error detail.
#[derive(Debug, Clone, Serialize)]
pub struct ApiErrorDetail {
    /// Error code (e.g., `SECRET_NOT_FOUND`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

impl ApiError {
    /// Creates a new API error.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ApiErrorDetail {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

/// Error codes used by the API.
pub mod error_codes {
    /// Missing or invalid Authorization header.
    pub const UNAUTHORIZED: &str = "UNAUTHORIZED";
    /// SVID signature verification failed.
    pub const INVALID_SVID: &str = "INVALID_SVID";
    /// SVID has expired.
    pub const SVID_EXPIRED: &str = "SVID_EXPIRED";
    /// Access denied by ABAC policy.
    pub const ACCESS_DENIED: &str = "ACCESS_DENIED";
    /// Secret not found.
    pub const SECRET_NOT_FOUND: &str = "SECRET_NOT_FOUND";
    /// Secret already exists.
    pub const SECRET_EXISTS: &str = "SECRET_EXISTS";
    /// Invalid scope value.
    pub const INVALID_SCOPE: &str = "INVALID_SCOPE";
    /// Invalid request body.
    pub const INVALID_REQUEST: &str = "INVALID_REQUEST";
    /// Internal server error.
    pub const INTERNAL_ERROR: &str = "INTERNAL_ERROR";
    /// Secret value exceeds maximum size limit.
    pub const SECRET_TOO_LARGE: &str = "SECRET_TOO_LARGE";
    /// Invalid service token.
    pub const INVALID_SERVICE_TOKEN: &str = "INVALID_SERVICE_TOKEN";
    /// Service token not configured.
    pub const SERVICE_TOKEN_NOT_CONFIGURED: &str = "SERVICE_TOKEN_NOT_CONFIGURED";
    /// Operation forbidden for this token type.
    pub const FORBIDDEN: &str = "FORBIDDEN";
}

/// Request to issue an SVID token.
///
/// In production, this would include the K8s `ServiceAccount` JWT.
/// For MVP, we accept principal info directly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRequest {
    /// Principal type (garage, bike, service).
    pub principal_type: PrincipalType,
    /// Principal identifier (garage-id, bike-id, or service name).
    pub principal_id: String,
    /// Optional pod UID for binding.
    pub pod_uid: Option<String>,
    /// Optional service name for bikes (required for service-scoped secret access via ABAC).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
}

/// Response containing an issued SVID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    /// The signed SVID JWT.
    pub token: String,
    /// Token expiration time (Unix timestamp).
    pub expires_at: i64,
}

/// Request to issue a garage SVID via moto-club delegation.
///
/// POST /auth/issue-garage-svid
///
/// This endpoint allows moto-club to request an SVID on behalf of a garage.
/// Garages don't have K8s API access, so moto-club fetches the SVID and
/// pushes it to the garage namespace as a Secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueGarageSvidRequest {
    /// The garage UUID.
    pub garage_id: String,
    /// The garage owner identifier.
    pub owner: String,
}

/// Response containing the issued garage SVID.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueGarageSvidResponse {
    /// The signed SVID JWT (1 hour TTL).
    pub token: String,
    /// Token expiration time (Unix timestamp).
    pub expires_at: i64,
    /// The SPIFFE ID for this garage.
    pub spiffe_id: String,
}

/// Request to create or update a secret.
#[derive(Debug, Clone, Deserialize)]
pub struct SetSecretRequest {
    /// The secret value (base64-encoded).
    pub value: String,
    /// Optional service name (for service-scoped secrets).
    pub service: Option<String>,
    /// Optional instance ID (for instance-scoped secrets).
    pub instance_id: Option<String>,
}

/// Response after creating or updating a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretResponse {
    /// Secret metadata.
    #[serde(flatten)]
    pub metadata: SecretMetadataResponse,
}

/// Secret metadata in API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMetadataResponse {
    /// Secret name.
    pub name: String,
    /// Secret scope.
    pub scope: Scope,
    /// Service (for service-scoped).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub service: Option<String>,
    /// Instance ID (for instance-scoped).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub instance_id: Option<String>,
    /// Current version number.
    pub version: u32,
    /// Creation timestamp (RFC 3339).
    pub created_at: String,
    /// Last update timestamp (RFC 3339).
    pub updated_at: String,
}

impl From<SecretMetadata> for SecretMetadataResponse {
    fn from(m: SecretMetadata) -> Self {
        Self {
            name: m.name,
            scope: m.scope,
            service: m.service,
            instance_id: m.instance_id,
            version: m.version,
            created_at: m.created_at.to_rfc3339(),
            updated_at: m.updated_at.to_rfc3339(),
        }
    }
}

/// Response containing a secret value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSecretResponse {
    /// Secret metadata.
    #[serde(flatten)]
    pub metadata: SecretMetadataResponse,
    /// The secret value (base64-encoded).
    pub value: String,
}

/// Response listing secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSecretsResponse {
    /// List of secret metadata (no values).
    pub secrets: Vec<SecretMetadataResponse>,
}

/// Response containing audit log entries.
#[derive(Debug, Clone, Serialize)]
pub struct AuditLogsResponse {
    /// List of audit log entries.
    pub entries: Vec<AuditEntryResponse>,
    /// Total number of entries (before pagination).
    pub total: usize,
}

/// An audit log entry in API responses (unified schema).
#[derive(Debug, Clone, Serialize)]
pub struct AuditEntryResponse {
    /// Unique identifier.
    pub id: String,
    /// Event category.
    pub event_type: AuditEventType,
    /// Which service produced the event.
    pub service: String,
    /// Principal type.
    pub principal_type: PrincipalType,
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
    /// Service-specific metadata.
    pub metadata: serde_json::Value,
    /// Source IP.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    /// When the event occurred (RFC 3339).
    pub timestamp: String,
}

impl From<&AuditEntry> for AuditEntryResponse {
    fn from(entry: &AuditEntry) -> Self {
        Self {
            id: entry.id.to_string(),
            event_type: entry.event_type,
            service: "keybox".to_string(),
            principal_type: entry.principal_type,
            principal_id: entry.principal_id.clone(),
            action: entry.action.clone(),
            resource_type: entry.resource_type.clone(),
            resource_id: entry.resource_id.clone(),
            outcome: entry.outcome.clone(),
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            client_ip: None,
            timestamp: entry.timestamp.to_rfc3339(),
        }
    }
}

/// Response after rotating a secret's DEK.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotateDekResponse {
    /// Secret name.
    pub name: String,
    /// Secret scope.
    pub scope: Scope,
    /// New version number after rotation.
    pub version: u32,
    /// When the rotation occurred (RFC 3339).
    pub rotated_at: String,
}

/// Query parameters for the rotate-dek endpoint.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RotateDekQuery {
    /// Scope in format: "global", "service/name", or "instance/id".
    pub scope: Option<String>,
}

/// Query parameters for audit log endpoint.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AuditLogsQuery {
    /// Filter by event type.
    pub event_type: Option<String>,
    /// Filter by principal ID.
    pub principal_id: Option<String>,
    /// Filter by resource type.
    pub resource_type: Option<String>,
    /// Filter by resource ID.
    pub resource_id: Option<String>,
    /// Maximum number of entries to return (default 100).
    pub limit: Option<usize>,
    /// Number of entries to skip (for pagination).
    pub offset: Option<usize>,
}

/// Extract SVID from Authorization header.
fn extract_svid(
    headers: &HeaderMap,
    validator: &SvidValidator,
) -> Result<SvidClaims, (StatusCode, Json<ApiError>)> {
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

    if token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::UNAUTHORIZED,
                "Empty Bearer token",
            )),
        ));
    }

    validator.validate(token).map_err(|e| match e {
        Error::SvidExpired => (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(error_codes::SVID_EXPIRED, "SVID has expired")),
        ),
        Error::InvalidSvidSignature => (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::INVALID_SVID,
                "Invalid SVID signature",
            )),
        ),
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(error_codes::INVALID_SVID, e.to_string())),
        ),
    })
}

/// Extract SVID from Authorization header with pod UID binding enforcement.
///
/// Like [`extract_svid`], but uses [`SvidValidator::validate_enforcing_pod_uid`]
/// to enforce pod UID binding when the SVID contains a `pod_uid` claim.
/// Use this for secret CRUD handlers (get, set, delete) per spec.
fn extract_svid_enforcing_pod_uid(
    headers: &HeaderMap,
    validator: &SvidValidator,
) -> Result<SvidClaims, (StatusCode, Json<ApiError>)> {
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

    if token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::UNAUTHORIZED,
                "Empty Bearer token",
            )),
        ));
    }

    validator
        .validate_enforcing_pod_uid(token)
        .map_err(|e| match e {
            Error::SvidExpired => (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(error_codes::SVID_EXPIRED, "SVID has expired")),
            ),
            Error::InvalidSvidSignature => (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    error_codes::INVALID_SVID,
                    "Invalid SVID signature",
                )),
            ),
            _ => (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(error_codes::INVALID_SVID, e.to_string())),
            ),
        })
}

/// Validate service token from Authorization header.
///
/// Returns `Ok(())` if the token matches the configured service token.
/// Returns an error if:
/// - No Authorization header
/// - Invalid header format
/// - Service token not configured
/// - Token doesn't match
fn validate_service_token(
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

    // Constant-time comparison to prevent timing attacks
    if token.as_bytes().ct_eq(expected.as_bytes()).into() {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::INVALID_SERVICE_TOKEN,
                "Invalid service token",
            )),
        ))
    }
}

/// Parse scope from path parameter.
fn parse_scope(scope_str: &str) -> Result<Scope, (StatusCode, Json<ApiError>)> {
    scope_str.parse::<Scope>().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                error_codes::INVALID_SCOPE,
                format!("Invalid scope: {scope_str}. Must be 'global', 'service', or 'instance'"),
            )),
        )
    })
}

/// Convert keybox error to API response.
///
/// Note: Both "secret not found" and "access denied" return 403 Forbidden with
/// the same `ACCESS_DENIED` error code to prevent secret enumeration attacks
/// (spec v0.4: attackers cannot determine which secrets exist based on response codes).
fn map_error(e: Error) -> (StatusCode, Json<ApiError>) {
    match e {
        Error::AccessDenied { .. } | Error::SecretNotFound { .. } => (
            // Return 403 for both "not found" and "access denied" to prevent
            // information leakage about secret existence (spec v0.4)
            StatusCode::FORBIDDEN,
            Json(ApiError::new(error_codes::ACCESS_DENIED, "Access denied")),
        ),
        Error::SecretExists { scope, name } => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                error_codes::SECRET_EXISTS,
                format!("Secret already exists: {scope}/{name}"),
            )),
        ),
        _ => {
            tracing::error!("Internal error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, "Internal error")),
            )
        }
    }
}

// =============================================================================
// Handlers
// =============================================================================

/// Issue an SVID token.
///
/// POST /auth/token
///
/// In production, this validates a K8s `ServiceAccount` JWT and issues an SVID.
/// For MVP, we accept principal info directly (no K8s validation).
async fn issue_token(
    State(state): State<AppState>,
    Json(req): Json<TokenRequest>,
) -> impl IntoResponse {
    let spiffe_id = match req.principal_type {
        PrincipalType::Garage => SpiffeId::garage(&req.principal_id),
        PrincipalType::Bike => SpiffeId::bike(&req.principal_id),
        PrincipalType::Service => SpiffeId::service(&req.principal_id),
    };

    let mut claims = SvidClaims::new(&spiffe_id, state.svid_issuer.ttl_secs());

    if let Some(pod_uid) = req.pod_uid {
        claims = claims.with_pod_uid(pod_uid);
    }

    if let Some(service) = req.service {
        claims = claims.with_service(service);
    }

    match state.svid_issuer.issue_with_claims(&claims) {
        Ok(token) => {
            // Audit SVID issuance (no secret values logged)
            let audit_entry = AuditEntry::svid_issued(&spiffe_id);
            state.repository.write().await.add_audit_entry(audit_entry);

            let response = TokenResponse {
                token,
                expires_at: claims.exp,
            };
            Ok((StatusCode::OK, Json(response)))
        }
        Err(e) => {
            tracing::error!("Failed to issue SVID: {e}");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "Failed to issue token",
                )),
            ))
        }
    }
}

/// Issue a garage SVID via moto-club delegation.
///
/// POST /auth/issue-garage-svid
///
/// This endpoint allows moto-club to request an SVID on behalf of a garage.
/// Authentication is via static service token (per spec v0.3).
/// Garage SVIDs have a 1-hour TTL (longer than the standard 15 min for bikes).
async fn issue_garage_svid(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<IssueGarageSvidRequest>,
) -> impl IntoResponse {
    // Validate service token (only moto-club can call this endpoint)
    validate_service_token(&headers, state.service_token.as_ref()).map_err(|_| {
        (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::FORBIDDEN,
                "Operation requires service token",
            )),
        )
    })?;

    // Create SPIFFE ID for the garage
    let spiffe_id = SpiffeId::garage(&req.garage_id);

    // Create claims with 1-hour TTL for garages
    let claims = SvidClaims::new(&spiffe_id, GARAGE_SVID_TTL_SECS);

    match state.svid_issuer.issue_with_claims(&claims) {
        Ok(token) => {
            // Audit SVID issuance with owner info
            tracing::info!(
                garage_id = %req.garage_id,
                owner = %req.owner,
                spiffe_id = %spiffe_id.to_uri(),
                expires_at = claims.exp,
                "issued garage SVID"
            );

            let audit_entry = AuditEntry::svid_issued(&spiffe_id);
            state.repository.write().await.add_audit_entry(audit_entry);

            let response = IssueGarageSvidResponse {
                token,
                expires_at: claims.exp,
                spiffe_id: spiffe_id.to_uri(),
            };
            Ok((StatusCode::OK, Json(response)))
        }
        Err(e) => {
            tracing::error!(error = %e, garage_id = %req.garage_id, "failed to issue garage SVID");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "Failed to issue garage SVID",
                )),
            ))
        }
    }
}

/// Get a secret value.
///
/// GET /secrets/{scope}/{name}
///
/// Accepts both service token (skip ABAC) and SVID (ABAC checked).
async fn get_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope_str, name)): Path<(String, String)>,
) -> impl IntoResponse {
    // Try service token first (skip ABAC), fall back to SVID + ABAC
    let claims = if validate_service_token(&headers, state.service_token.as_ref()).is_ok() {
        state.service_token_claims()
    } else {
        extract_svid_enforcing_pod_uid(&headers, &state.svid_validator)?
    };
    let scope = parse_scope(&scope_str)?;

    let mut repo = state.repository.write().await;

    // Get value based on scope
    let (value, metadata) = match scope {
        Scope::Global => {
            let value = repo.get(&claims, scope, &name).map_err(map_error)?;
            // Get metadata by listing (since get doesn't return it)
            let list = repo.list(&claims, scope);
            // Return 403 for "not found" to prevent secret enumeration (spec v0.4)
            let meta = list.into_iter().find(|m| m.name == name).ok_or_else(|| {
                (
                    StatusCode::FORBIDDEN,
                    Json(ApiError::new(error_codes::ACCESS_DENIED, "Access denied")),
                )
            })?;
            (value, meta)
        }
        Scope::Service | Scope::Instance => {
            // For service/instance scope, name format is "context/secret-name"
            // e.g., "tokenization/db-password" or "garage-123/dev-token"
            let (context, secret_name) = name.split_once('/').ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError::new(
                        error_codes::INVALID_REQUEST,
                        format!("For {scope} scope, name must be in format 'context/secret-name'"),
                    )),
                )
            })?;

            let value = if scope == Scope::Service {
                repo.get_service(&claims, context, secret_name)
                    .map_err(map_error)?
            } else {
                repo.get_instance(&claims, context, secret_name)
                    .map_err(map_error)?
            };

            // Get metadata by listing
            let list = if scope == Scope::Service {
                repo.list_service(&claims, context)
            } else {
                repo.list_instance(&claims, context)
            };
            // Return 403 for "not found" to prevent secret enumeration (spec v0.4)
            let meta = list
                .into_iter()
                .find(|m| m.name == secret_name)
                .ok_or_else(|| {
                    (
                        StatusCode::FORBIDDEN,
                        Json(ApiError::new(error_codes::ACCESS_DENIED, "Access denied")),
                    )
                })?;
            (value, meta)
        }
    };

    let response = GetSecretResponse {
        metadata: SecretMetadataResponse::from(metadata),
        value: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &value),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Create or update a secret.
///
/// POST /secrets/{scope}/{name}
///
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
async fn set_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope_str, name)): Path<(String, String)>,
    Json(req): Json<SetSecretRequest>,
) -> impl IntoResponse {
    validate_service_token(&headers, state.service_token.as_ref()).map_err(|_| {
        (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::FORBIDDEN,
                "Operation requires service token",
            )),
        )
    })?;
    let claims = state.service_token_claims();
    let scope = parse_scope(&scope_str)?;

    // Decode base64 value
    let value = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &req.value)
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    error_codes::INVALID_REQUEST,
                    "Invalid base64 encoding for value",
                )),
            )
        })?;

    // Validate secret size (1 MB limit per spec)
    if value.len() > MAX_SECRET_SIZE_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                error_codes::SECRET_TOO_LARGE,
                format!(
                    "Secret value exceeds maximum size of {} bytes (got {} bytes)",
                    MAX_SECRET_SIZE_BYTES,
                    value.len()
                ),
            )),
        ));
    }

    let mut repo = state.repository.write().await;

    // Try to create, if exists try to update
    let metadata = match scope {
        Scope::Global => {
            match repo.create(&claims, scope, &name, &value) {
                Ok(meta) => meta,
                Err(Error::SecretExists { .. }) => {
                    // Update existing
                    repo.update(&claims, scope, &name, &value)
                        .map_err(map_error)?
                }
                Err(e) => return Err(map_error(e)),
            }
        }
        Scope::Service => {
            let service = req.service.as_deref().or_else(|| name.split('/').next());
            let secret_name = name.split('/').nth(1).unwrap_or(&name);
            let service = service.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError::new(
                        error_codes::INVALID_REQUEST,
                        "Service name required for service-scoped secrets",
                    )),
                )
            })?;

            match repo.create_service(&claims, service, secret_name, &value) {
                Ok(meta) => meta,
                Err(Error::SecretExists { .. }) => repo
                    .update_service(&claims, service, secret_name, &value)
                    .map_err(map_error)?,
                Err(e) => return Err(map_error(e)),
            }
        }
        Scope::Instance => {
            let instance_id = req
                .instance_id
                .as_deref()
                .or_else(|| name.split('/').next());
            let secret_name = name.split('/').nth(1).unwrap_or(&name);
            let instance_id = instance_id.ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError::new(
                        error_codes::INVALID_REQUEST,
                        "Instance ID required for instance-scoped secrets",
                    )),
                )
            })?;

            match repo.create_instance(&claims, instance_id, secret_name, &value) {
                Ok(meta) => meta,
                Err(Error::SecretExists { .. }) => repo
                    .update_instance(&claims, instance_id, secret_name, &value)
                    .map_err(map_error)?,
                Err(e) => return Err(map_error(e)),
            }
        }
    };

    let response = SecretResponse {
        metadata: SecretMetadataResponse::from(metadata),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Delete a secret.
///
/// DELETE /secrets/{scope}/{name}
///
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
async fn delete_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope_str, name)): Path<(String, String)>,
) -> impl IntoResponse {
    validate_service_token(&headers, state.service_token.as_ref()).map_err(|_| {
        (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::FORBIDDEN,
                "Operation requires service token",
            )),
        )
    })?;
    let claims = state.service_token_claims();
    let scope = parse_scope(&scope_str)?;

    {
        let mut repo = state.repository.write().await;

        match scope {
            Scope::Global => {
                repo.delete(&claims, scope, &name).map_err(map_error)?;
            }
            Scope::Service | Scope::Instance => {
                let (context, secret_name) = name.split_once('/').ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(ApiError::new(
                            error_codes::INVALID_REQUEST,
                            format!(
                                "For {scope} scope, name must be in format 'context/secret-name'"
                            ),
                        )),
                    )
                })?;

                if scope == Scope::Service {
                    repo.delete_service(&claims, context, secret_name)
                        .map_err(map_error)?;
                } else {
                    repo.delete_instance(&claims, context, secret_name)
                        .map_err(map_error)?;
                }
            }
        }
    }

    Ok::<_, (StatusCode, Json<ApiError>)>(StatusCode::NO_CONTENT)
}

/// List secrets in a scope.
///
/// GET /secrets/{scope}
///
/// Accepts both service token (return all in scope) and SVID (ABAC filtered).
async fn list_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_str): Path<String>,
) -> impl IntoResponse {
    // Try service token first (return all), fall back to SVID + ABAC
    let claims = if validate_service_token(&headers, state.service_token.as_ref()).is_ok() {
        state.service_token_claims()
    } else {
        extract_svid(&headers, &state.svid_validator)?
    };
    let scope = parse_scope(&scope_str)?;

    let secrets = state.repository.read().await.list(&claims, scope);

    let response = ListSecretsResponse {
        secrets: secrets
            .into_iter()
            .map(SecretMetadataResponse::from)
            .collect(),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// List secrets for a specific service.
///
/// GET /secrets/service/{service}
///
/// Accepts both service token (return all) and SVID (ABAC filtered).
async fn list_service_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service): Path<String>,
) -> impl IntoResponse {
    let claims = if validate_service_token(&headers, state.service_token.as_ref()).is_ok() {
        state.service_token_claims()
    } else {
        extract_svid(&headers, &state.svid_validator)?
    };

    let secrets = state
        .repository
        .read()
        .await
        .list_service(&claims, &service);

    let response = ListSecretsResponse {
        secrets: secrets
            .into_iter()
            .map(SecretMetadataResponse::from)
            .collect(),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// List secrets for a specific instance.
///
/// `GET /secrets/instance/{instance_id}`
///
/// Accepts both service token (return all) and SVID (ABAC filtered).
async fn list_instance_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> impl IntoResponse {
    let claims = if validate_service_token(&headers, state.service_token.as_ref()).is_ok() {
        state.service_token_claims()
    } else {
        extract_svid(&headers, &state.svid_validator)?
    };

    let secrets = state
        .repository
        .read()
        .await
        .list_instance(&claims, &instance_id);

    let response = ListSecretsResponse {
        secrets: secrets
            .into_iter()
            .map(SecretMetadataResponse::from)
            .collect(),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Query audit logs.
///
/// GET /audit/logs
///
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
async fn get_audit_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AuditLogsQuery>,
) -> impl IntoResponse {
    validate_service_token(&headers, state.service_token.as_ref()).map_err(|_| {
        (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::FORBIDDEN,
                "Operation requires service token",
            )),
        )
    })?;

    // Clone audit entries to release the lock quickly
    let all_entries: Vec<AuditEntry> = state.repository.read().await.audit_log().to_vec();

    // Apply filters
    let filtered: Vec<&AuditEntry> = all_entries
        .iter()
        .filter(|e| {
            // Filter by event type
            if let Some(ref event_type_str) = query.event_type {
                let matches = match event_type_str.as_str() {
                    "secret_accessed" => e.event_type == AuditEventType::SecretAccessed,
                    "secret_created" => e.event_type == AuditEventType::SecretCreated,
                    "secret_updated" => e.event_type == AuditEventType::SecretUpdated,
                    "secret_deleted" => e.event_type == AuditEventType::SecretDeleted,
                    "svid_issued" => e.event_type == AuditEventType::SvidIssued,
                    "auth_failed" => e.event_type == AuditEventType::AuthFailed,
                    "access_denied" => e.event_type == AuditEventType::AccessDenied,
                    "dek_rotated" => e.event_type == AuditEventType::DekRotated,
                    _ => true,
                };
                if !matches {
                    return false;
                }
            }

            // Filter by principal ID
            if let Some(ref pid) = query.principal_id
                && &e.principal_id != pid
            {
                return false;
            }

            // Filter by resource type
            if let Some(ref rt) = query.resource_type
                && &e.resource_type != rt
            {
                return false;
            }

            true
        })
        .collect();

    let total = filtered.len();

    // Apply pagination
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);

    let entries: Vec<AuditEntryResponse> = filtered
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(AuditEntryResponse::from)
        .collect();

    let response = AuditLogsResponse { entries, total };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Rotate a secret's DEK.
///
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
async fn rotate_dek(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Query(query): Query<RotateDekQuery>,
) -> impl IntoResponse {
    validate_service_token(&headers, state.service_token.as_ref()).map_err(|_| {
        (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::FORBIDDEN,
                "Operation requires service token",
            )),
        )
    })?;
    let claims = state.service_token_claims();

    let scope_str = query.scope.as_deref().unwrap_or("global");
    let (scope, service, instance_id) = parse_scope_query(scope_str)?;

    let metadata = state
        .repository
        .write()
        .await
        .rotate_dek(
            &claims,
            scope,
            service.as_deref(),
            instance_id.as_deref(),
            &name,
        )
        .map_err(|e| {
            if let Error::SecretNotFound { .. } = e {
                (
                    StatusCode::NOT_FOUND,
                    Json(ApiError::new(
                        error_codes::SECRET_NOT_FOUND,
                        format!("Secret not found: {scope_str}/{name}"),
                    )),
                )
            } else {
                tracing::error!("DEK rotation failed: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(error_codes::INTERNAL_ERROR, "Internal error")),
                )
            }
        })?;

    let response = RotateDekResponse {
        name: metadata.name,
        scope: metadata.scope,
        version: metadata.version,
        rotated_at: metadata.updated_at.to_rfc3339(),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Parse scope query parameter.
///
/// Accepts: "global", "service/name", "instance/id".
///
/// # Errors
///
/// Returns a `(StatusCode, Json<ApiError>)` tuple if the scope string is invalid.
#[allow(clippy::type_complexity)]
pub fn parse_scope_query(
    scope_str: &str,
) -> Result<(Scope, Option<String>, Option<String>), (StatusCode, Json<ApiError>)> {
    if scope_str == "global" {
        return Ok((Scope::Global, None, None));
    }

    if let Some(service) = scope_str.strip_prefix("service/") {
        if service.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    error_codes::INVALID_SCOPE,
                    "Service name required in scope parameter (e.g., scope=service/club)",
                )),
            ));
        }
        return Ok((Scope::Service, Some(service.to_string()), None));
    }

    if let Some(instance_id) = scope_str.strip_prefix("instance/") {
        if instance_id.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    error_codes::INVALID_SCOPE,
                    "Instance ID required in scope parameter (e.g., scope=instance/abc123)",
                )),
            ));
        }
        return Ok((Scope::Instance, None, Some(instance_id.to_string())));
    }

    Err((
        StatusCode::BAD_REQUEST,
        Json(ApiError::new(
            error_codes::INVALID_SCOPE,
            format!(
                "Invalid scope: {scope_str}. Must be 'global', 'service/{{name}}', or 'instance/{{id}}'"
            ),
        )),
    ))
}

// =============================================================================
// Router
// =============================================================================

/// Creates the keybox API router.
///
/// Includes:
/// - `POST /auth/token` - Issue SVID token (bikes, via K8s SA JWT)
/// - `POST /auth/issue-garage-svid` - Issue garage SVID (moto-club delegation)
/// - `GET /secrets/{scope}/{name}` - Get secret value
/// - `POST /secrets/{scope}/{name}` - Create/update secret
/// - `DELETE /secrets/{scope}/{name}` - Delete secret
/// - `GET /secrets/{scope}` - List secrets (global scope only)
/// - `GET /secrets/service/{service}` - List service secrets
/// - `GET /secrets/instance/{instance_id}` - List instance secrets
/// - `GET /audit/logs` - Query audit logs (admin services only)
/// - `POST /admin/rotate-dek/{name}` - Rotate a secret's DEK (admin only)
pub fn router(state: AppState) -> Router {
    Router::new()
        // Auth endpoints
        .route("/auth/token", post(issue_token))
        .route("/auth/issue-garage-svid", post(issue_garage_svid))
        // Secret CRUD endpoints
        .route(
            "/secrets/{scope}/{name}",
            get(get_secret).post(set_secret).delete(delete_secret),
        )
        // List endpoints
        .route("/secrets/{scope}", get(list_secrets))
        .route("/secrets/service/{service}", get(list_service_secrets))
        .route(
            "/secrets/instance/{instance_id}",
            get(list_instance_secrets),
        )
        // Audit endpoint
        .route("/audit/logs", get(get_audit_logs))
        // Admin endpoints
        .route("/admin/rotate-dek/{name}", post(rotate_dek))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_serialization() {
        let error = ApiError::new("SECRET_NOT_FOUND", "Secret 'ai/anthropic' not found");
        let json = serde_json::to_string(&error).unwrap();

        assert!(json.contains(r#""code":"SECRET_NOT_FOUND""#));
        assert!(json.contains(r#""message":"Secret 'ai/anthropic' not found""#));
    }

    #[test]
    fn token_request_deserialize() {
        let json = r#"{"principal_type":"garage","principal_id":"my-garage"}"#;
        let req: TokenRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.principal_type, PrincipalType::Garage);
        assert_eq!(req.principal_id, "my-garage");
        assert!(req.pod_uid.is_none());
    }

    #[test]
    fn token_request_with_pod_uid() {
        let json = r#"{"principal_type":"bike","principal_id":"bike-123","pod_uid":"pod-abc-123"}"#;
        let req: TokenRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.principal_type, PrincipalType::Bike);
        assert_eq!(req.principal_id, "bike-123");
        assert_eq!(req.pod_uid, Some("pod-abc-123".to_string()));
    }

    #[test]
    fn token_response_serialize() {
        let resp = TokenResponse {
            token: "eyJ...".to_string(),
            expires_at: 1_700_000_000,
        };
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains(r#""token":"eyJ...""#));
        assert!(json.contains(r#""expires_at":1700000000"#));
    }

    #[test]
    fn set_secret_request_deserialize() {
        let json = r#"{"value":"c2VjcmV0","service":"tokenization"}"#;
        let req: SetSecretRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.value, "c2VjcmV0");
        assert_eq!(req.service, Some("tokenization".to_string()));
        assert!(req.instance_id.is_none());
    }

    #[test]
    fn secret_metadata_response_serialize() {
        let meta = SecretMetadata::global("ai/anthropic");
        let resp = SecretMetadataResponse::from(meta);
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains(r#""name":"ai/anthropic""#));
        assert!(json.contains(r#""scope":"global""#));
        assert!(json.contains(r#""version":1"#));
        // service and instance_id should be omitted when None
        assert!(!json.contains("service"));
        assert!(!json.contains("instance_id"));
    }

    #[test]
    fn secret_metadata_response_with_service() {
        let meta = SecretMetadata::service("tokenization", "db/password");
        let resp = SecretMetadataResponse::from(meta);
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains(r#""scope":"service""#));
        assert!(json.contains(r#""service":"tokenization""#));
    }

    #[test]
    fn list_secrets_response_serialize() {
        let secrets = vec![
            SecretMetadataResponse::from(SecretMetadata::global("ai/anthropic")),
            SecretMetadataResponse::from(SecretMetadata::global("ai/openai")),
        ];
        let resp = ListSecretsResponse { secrets };
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains(r#""secrets":"#));
        assert!(json.contains("ai/anthropic"));
        assert!(json.contains("ai/openai"));
    }

    #[test]
    fn get_secret_response_serialize() {
        let meta = SecretMetadata::global("ai/anthropic");
        let resp = GetSecretResponse {
            metadata: SecretMetadataResponse::from(meta),
            value: "c2VjcmV0".to_string(), // "secret" in base64
        };
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains(r#""name":"ai/anthropic""#));
        assert!(json.contains(r#""value":"c2VjcmV0""#));
    }

    #[test]
    fn parse_scope_valid() {
        assert!(parse_scope("global").is_ok());
        assert!(parse_scope("service").is_ok());
        assert!(parse_scope("instance").is_ok());
    }

    #[test]
    fn parse_scope_invalid() {
        let result = parse_scope("invalid");
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn audit_entry_response_from_entry() {
        let spiffe = SpiffeId::garage("test-garage");
        let entry = AuditEntry::svid_issued(&spiffe);
        let resp = AuditEntryResponse::from(&entry);

        assert_eq!(resp.event_type, AuditEventType::SvidIssued);
        assert_eq!(resp.principal_type, PrincipalType::Garage);
        assert_eq!(resp.principal_id, "spiffe://moto.local/garage/test-garage");
        assert_eq!(resp.resource_type, "svid");
        assert_eq!(resp.action, "create");
    }

    #[test]
    fn audit_logs_response_serialize() {
        let spiffe = SpiffeId::service("moto-club");
        let entry = AuditEntry::svid_issued(&spiffe);
        let resp = AuditLogsResponse {
            entries: vec![AuditEntryResponse::from(&entry)],
            total: 1,
        };
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains(r#""event_type":"svid_issued""#));
        assert!(json.contains(r#""total":1"#));
        assert!(json.contains(r#""principal_id":"spiffe://moto.local/service/moto-club""#));
    }

    #[test]
    fn audit_entry_response_access_denied() {
        let spiffe = SpiffeId::bike("untrusted-bike");
        let entry = AuditEntry::access_denied(&spiffe, Scope::Global, "crypto/master-key");
        let resp = AuditEntryResponse::from(&entry);

        assert_eq!(resp.event_type, AuditEventType::AccessDenied);
        assert_eq!(resp.resource_type, "secret");
        assert_eq!(resp.resource_id, "global/crypto/master-key");
        assert_eq!(resp.outcome, "denied");
    }

    #[test]
    fn audit_logs_query_deserialize() {
        let query: AuditLogsQuery = serde_json::from_str(
            r#"{
            "event_type": "secret_accessed",
            "principal_id": "my-garage",
            "resource_type": "secret",
            "limit": 50,
            "offset": 10
        }"#,
        )
        .unwrap();

        assert_eq!(query.event_type, Some("secret_accessed".to_string()));
        assert_eq!(query.principal_id, Some("my-garage".to_string()));
        assert_eq!(query.resource_type, Some("secret".to_string()));
        assert_eq!(query.limit, Some(50));
        assert_eq!(query.offset, Some(10));
    }

    #[test]
    fn issue_garage_svid_request_deserialize() {
        let json = r#"{"garage_id":"abc123","owner":"user@example.com"}"#;
        let req: IssueGarageSvidRequest = serde_json::from_str(json).unwrap();

        assert_eq!(req.garage_id, "abc123");
        assert_eq!(req.owner, "user@example.com");
    }

    #[test]
    fn issue_garage_svid_response_serialize() {
        let resp = IssueGarageSvidResponse {
            token: "eyJ...".to_string(),
            expires_at: 1_700_003_600,
            spiffe_id: "spiffe://moto.local/garage/abc123".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();

        assert!(json.contains(r#""token":"eyJ...""#));
        assert!(json.contains(r#""expires_at":1700003600"#));
        assert!(json.contains(r#""spiffe_id":"spiffe://moto.local/garage/abc123""#));
    }

    #[test]
    fn garage_svid_ttl_is_one_hour() {
        assert_eq!(GARAGE_SVID_TTL_SECS, 3600);
    }

    #[test]
    fn validate_service_token_missing_header() {
        let headers = HeaderMap::new();
        let token = Some("secret-token".to_string());

        let result = validate_service_token(&headers, token.as_ref());
        assert!(result.is_err());
        let (status, json) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json.0.error.code, error_codes::UNAUTHORIZED);
    }

    #[test]
    fn validate_service_token_invalid_format() {
        use axum::http::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Basic abc123"));
        let token = Some("secret-token".to_string());

        let result = validate_service_token(&headers, token.as_ref());
        assert!(result.is_err());
        let (status, json) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json.0.error.code, error_codes::UNAUTHORIZED);
    }

    #[test]
    fn validate_service_token_not_configured() {
        use axum::http::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer some-token"),
        );
        let token: Option<String> = None;

        let result = validate_service_token(&headers, token.as_ref());
        assert!(result.is_err());
        let (status, json) = result.unwrap_err();
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(json.0.error.code, error_codes::SERVICE_TOKEN_NOT_CONFIGURED);
    }

    #[test]
    fn validate_service_token_wrong_token() {
        use axum::http::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer wrong-token"),
        );
        let token = Some("correct-token".to_string());

        let result = validate_service_token(&headers, token.as_ref());
        assert!(result.is_err());
        let (status, json) = result.unwrap_err();
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(json.0.error.code, error_codes::INVALID_SERVICE_TOKEN);
    }

    #[test]
    fn validate_service_token_success() {
        use axum::http::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer secret-token"),
        );
        let token = Some("secret-token".to_string());

        let result = validate_service_token(&headers, token.as_ref());
        assert!(result.is_ok());
    }

    #[test]
    fn validate_service_token_case_insensitive_bearer() {
        use axum::http::HeaderValue;

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static("bearer secret-token"),
        );
        let token = Some("secret-token".to_string());

        let result = validate_service_token(&headers, token.as_ref());
        assert!(result.is_ok());
    }

    #[test]
    fn max_secret_size_is_one_mb() {
        assert_eq!(MAX_SECRET_SIZE_BYTES, 1_048_576);
    }

    #[test]
    fn secret_too_large_error_code_exists() {
        assert_eq!(error_codes::SECRET_TOO_LARGE, "SECRET_TOO_LARGE");
    }
}
