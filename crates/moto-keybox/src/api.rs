//! REST API endpoints for moto-keybox.
//!
//! Provides HTTP API layer for keybox:
//! - `POST /auth/token` - Exchange K8s `ServiceAccount` JWT for SVID
//! - `GET /secrets/{scope}/{name}` - Retrieve a secret
//! - `POST /secrets/{scope}/{name}` - Create/update a secret
//! - `DELETE /secrets/{scope}/{name}` - Delete a secret
//! - `GET /secrets/{scope}` - List secrets in a scope
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
    extract::{Path, State},
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
use crate::types::{PrincipalType, Scope, SecretMetadata, SpiffeId};

/// Shared application state for the keybox API.
#[derive(Clone)]
pub struct AppState {
    /// Secret repository (thread-safe).
    pub repository: Arc<RwLock<SecretRepository>>,
    /// SVID issuer for token generation.
    pub svid_issuer: Arc<SvidIssuer>,
    /// SVID validator for token verification.
    pub svid_validator: Arc<SvidValidator>,
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
        }
    }

    /// Creates `AppState` from an existing repository.
    #[must_use]
    pub fn with_repository(
        repository: SecretRepository,
        svid_issuer: SvidIssuer,
        svid_validator: SvidValidator,
    ) -> Self {
        Self {
            repository: Arc::new(RwLock::new(repository)),
            svid_issuer: Arc::new(svid_issuer),
            svid_validator: Arc::new(svid_validator),
        }
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
}

/// Request to issue an SVID token.
///
/// In production, this would include the K8s `ServiceAccount` JWT.
/// For MVP, we accept principal info directly.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenRequest {
    /// Principal type (garage, bike, service).
    pub principal_type: PrincipalType,
    /// Principal identifier (garage-id, bike-id, or service name).
    pub principal_id: String,
    /// Optional pod UID for binding.
    pub pod_uid: Option<String>,
}

/// Response containing an issued SVID.
#[derive(Debug, Clone, Serialize)]
pub struct TokenResponse {
    /// The signed SVID JWT.
    pub token: String,
    /// Token expiration time (Unix timestamp).
    pub expires_at: i64,
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
#[derive(Debug, Clone, Serialize)]
pub struct SecretResponse {
    /// Secret metadata.
    #[serde(flatten)]
    pub metadata: SecretMetadataResponse,
}

/// Secret metadata in API responses.
#[derive(Debug, Clone, Serialize)]
pub struct SecretMetadataResponse {
    /// Secret name.
    pub name: String,
    /// Secret scope.
    pub scope: Scope,
    /// Service (for service-scoped).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Instance ID (for instance-scoped).
    #[serde(skip_serializing_if = "Option::is_none")]
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
#[derive(Debug, Clone, Serialize)]
pub struct GetSecretResponse {
    /// Secret metadata.
    #[serde(flatten)]
    pub metadata: SecretMetadataResponse,
    /// The secret value (base64-encoded).
    pub value: String,
}

/// Response listing secrets.
#[derive(Debug, Clone, Serialize)]
pub struct ListSecretsResponse {
    /// List of secret metadata (no values).
    pub secrets: Vec<SecretMetadataResponse>,
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
fn map_error(e: Error) -> (StatusCode, Json<ApiError>) {
    match e {
        Error::AccessDenied { message } => (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(error_codes::ACCESS_DENIED, message)),
        ),
        Error::SecretNotFound { scope, name } => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                error_codes::SECRET_NOT_FOUND,
                format!("Secret not found: {scope}/{name}"),
            )),
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

    let mut claims = SvidClaims::new(&spiffe_id, crate::svid::DEFAULT_SVID_TTL_SECS);

    if let Some(pod_uid) = req.pod_uid {
        claims = claims.with_pod_uid(pod_uid);
    }

    match state.svid_issuer.issue_with_claims(&claims) {
        Ok(token) => {
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

/// Get a secret value.
///
/// GET /secrets/{scope}/{name}
async fn get_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope_str, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let claims = extract_svid(&headers, &state.svid_validator)?;
    let scope = parse_scope(&scope_str)?;

    let mut repo = state.repository.write().await;

    // Get value based on scope
    let (value, metadata) = match scope {
        Scope::Global => {
            let value = repo.get(&claims, scope, &name).map_err(map_error)?;
            // Get metadata by listing (since get doesn't return it)
            let list = repo.list(&claims, scope);
            let meta = list.into_iter().find(|m| m.name == name).ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(ApiError::new(
                        error_codes::SECRET_NOT_FOUND,
                        format!("Secret not found: {scope}/{name}"),
                    )),
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
            let meta = list
                .into_iter()
                .find(|m| m.name == secret_name)
                .ok_or_else(|| {
                    (
                        StatusCode::NOT_FOUND,
                        Json(ApiError::new(
                            error_codes::SECRET_NOT_FOUND,
                            format!("Secret not found: {scope}/{name}"),
                        )),
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
async fn set_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope_str, name)): Path<(String, String)>,
    Json(req): Json<SetSecretRequest>,
) -> impl IntoResponse {
    let claims = extract_svid(&headers, &state.svid_validator)?;
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
async fn delete_secret(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((scope_str, name)): Path<(String, String)>,
) -> impl IntoResponse {
    let claims = extract_svid(&headers, &state.svid_validator)?;
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
async fn list_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(scope_str): Path<String>,
) -> impl IntoResponse {
    let claims = extract_svid(&headers, &state.svid_validator)?;
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
async fn list_service_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(service): Path<String>,
) -> impl IntoResponse {
    let claims = extract_svid(&headers, &state.svid_validator)?;

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
async fn list_instance_secrets(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> impl IntoResponse {
    let claims = extract_svid(&headers, &state.svid_validator)?;

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

// =============================================================================
// Router
// =============================================================================

/// Creates the keybox API router.
///
/// Includes:
/// - `POST /auth/token` - Issue SVID token
/// - `GET /secrets/{scope}/{name}` - Get secret value
/// - `POST /secrets/{scope}/{name}` - Create/update secret
/// - `DELETE /secrets/{scope}/{name}` - Delete secret
/// - `GET /secrets/{scope}` - List secrets (global scope only)
/// - `GET /secrets/service/{service}` - List service secrets
/// - `GET /secrets/instance/{instance_id}` - List instance secrets
pub fn router(state: AppState) -> Router {
    Router::new()
        // Auth endpoint
        .route("/auth/token", post(issue_token))
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
            expires_at: 1700000000,
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
}
