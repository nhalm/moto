//! PostgreSQL-backed REST API endpoints for moto-keybox.
//!
//! This module provides the same API as `api` but uses `PostgreSQL` storage
//! via `PgSecretRepository` instead of in-memory storage.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use moto_keybox_db::{AuditLogQuery, DbPool, audit_repo};

use crate::Error;
use crate::abac::PolicyEngine;
use crate::api::{
    ApiError, AuditEntryResponse, AuditLogsQuery, AuditLogsResponse, GARAGE_SVID_TTL_SECS,
    GetSecretResponse, IssueGarageSvidRequest, IssueGarageSvidResponse, ListSecretsResponse,
    MAX_SECRET_SIZE_BYTES, RotateDekQuery, RotateDekResponse, SecretMetadataResponse,
    SecretResponse, SetSecretRequest, TokenRequest, TokenResponse, error_codes, parse_scope_query,
};
use crate::envelope::MasterKey;
use crate::pg_repository::PgSecretRepository;
use crate::svid::{SvidClaims, SvidIssuer, SvidValidator};
use crate::types::{AuditEntry, AuditEventType, PrincipalType, Scope, SpiffeId};

/// Shared application state for the PostgreSQL-backed keybox API.
#[derive(Clone)]
pub struct PgAppState {
    /// PostgreSQL-backed secret repository.
    pub repository: Arc<PgSecretRepository>,
    /// SVID issuer for token generation.
    pub svid_issuer: Arc<SvidIssuer>,
    /// SVID validator for token verification.
    pub svid_validator: Arc<SvidValidator>,
    /// Service token for moto-club authentication.
    service_token: Option<String>,
    /// Admin service name for creating synthetic claims on service token auth.
    admin_service: String,
}

impl PgAppState {
    /// Creates a new `PgAppState` with `PostgreSQL` backend.
    #[must_use]
    pub fn new(
        pool: DbPool,
        master_key: MasterKey,
        svid_issuer: SvidIssuer,
        svid_validator: SvidValidator,
        admin_service: &str,
    ) -> Self {
        let policy = PolicyEngine::new().with_admin_service(admin_service);
        let repository = PgSecretRepository::new(pool, master_key, policy);
        Self {
            repository: Arc::new(repository),
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

    /// Returns a reference to the database pool for health checks.
    #[must_use]
    pub fn pool(&self) -> &DbPool {
        self.repository.pool()
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
fn map_error(e: Error) -> (StatusCode, Json<ApiError>) {
    match e {
        Error::AccessDenied { .. } | Error::SecretNotFound { .. } => (
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
async fn issue_token(
    State(state): State<PgAppState>,
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

    match state.svid_issuer.issue_with_claims(&claims) {
        Ok(token) => {
            let audit_entry = AuditEntry::svid_issued(&spiffe_id);
            state.repository.add_audit_entry(&audit_entry).await;

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
async fn issue_garage_svid(
    State(state): State<PgAppState>,
    headers: HeaderMap,
    Json(req): Json<IssueGarageSvidRequest>,
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

    let spiffe_id = SpiffeId::garage(&req.garage_id);
    let claims = SvidClaims::new(&spiffe_id, GARAGE_SVID_TTL_SECS);

    match state.svid_issuer.issue_with_claims(&claims) {
        Ok(token) => {
            tracing::info!(
                garage_id = %req.garage_id,
                owner = %req.owner,
                spiffe_id = %spiffe_id.to_uri(),
                expires_at = claims.exp,
                "issued garage SVID"
            );

            let audit_entry = AuditEntry::svid_issued(&spiffe_id);
            state.repository.add_audit_entry(&audit_entry).await;

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
/// Accepts both service token (skip ABAC) and SVID (ABAC checked).
async fn get_secret(
    State(state): State<PgAppState>,
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

    let value = match scope {
        Scope::Global => state.repository.get(&claims, scope, &name).await,
        Scope::Service | Scope::Instance => {
            let (context, secret_name) = name.split_once('/').ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError::new(
                        error_codes::INVALID_REQUEST,
                        format!("For {scope} scope, name must be in format 'context/secret-name'"),
                    )),
                )
            })?;

            if scope == Scope::Service {
                state
                    .repository
                    .get_service(&claims, context, secret_name)
                    .await
            } else {
                state
                    .repository
                    .get_instance(&claims, context, secret_name)
                    .await
            }
        }
    }
    .map_err(map_error)?;

    // Get metadata for response
    let metadata_list = match scope {
        Scope::Global => state.repository.list(&claims, scope).await,
        Scope::Service => {
            let context = name.split('/').next().unwrap_or("");
            state.repository.list_service(&claims, context).await
        }
        Scope::Instance => {
            let context = name.split('/').next().unwrap_or("");
            state.repository.list_instance(&claims, context).await
        }
    };

    let secret_name_part = name.split('/').nth(1).unwrap_or(&name);
    let metadata = metadata_list
        .into_iter()
        .find(|m| m.name == secret_name_part || m.name == name)
        .ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(ApiError::new(error_codes::ACCESS_DENIED, "Access denied")),
            )
        })?;

    let response = GetSecretResponse {
        metadata: SecretMetadataResponse::from(metadata),
        value: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &value),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Create or update a secret.
///
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
#[allow(clippy::too_many_lines)]
async fn set_secret(
    State(state): State<PgAppState>,
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

    let metadata = match scope {
        Scope::Global => match state.repository.create(&claims, scope, &name, &value).await {
            Ok(meta) => meta,
            Err(Error::SecretExists { .. }) => state
                .repository
                .update(&claims, scope, &name, &value)
                .await
                .map_err(map_error)?,
            Err(e) => return Err(map_error(e)),
        },
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

            match state
                .repository
                .create_service(&claims, service, secret_name, &value)
                .await
            {
                Ok(meta) => meta,
                Err(Error::SecretExists { .. }) => state
                    .repository
                    .update_service(&claims, service, secret_name, &value)
                    .await
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

            match state
                .repository
                .create_instance(&claims, instance_id, secret_name, &value)
                .await
            {
                Ok(meta) => meta,
                Err(Error::SecretExists { .. }) => state
                    .repository
                    .update_instance(&claims, instance_id, secret_name, &value)
                    .await
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
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
async fn delete_secret(
    State(state): State<PgAppState>,
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

    match scope {
        Scope::Global => {
            state
                .repository
                .delete(&claims, scope, &name)
                .await
                .map_err(map_error)?;
        }
        Scope::Service | Scope::Instance => {
            let (context, secret_name) = name.split_once('/').ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError::new(
                        error_codes::INVALID_REQUEST,
                        format!("For {scope} scope, name must be in format 'context/secret-name'"),
                    )),
                )
            })?;

            if scope == Scope::Service {
                state
                    .repository
                    .delete_service(&claims, context, secret_name)
                    .await
                    .map_err(map_error)?;
            } else {
                state
                    .repository
                    .delete_instance(&claims, context, secret_name)
                    .await
                    .map_err(map_error)?;
            }
        }
    }

    Ok::<_, (StatusCode, Json<ApiError>)>(StatusCode::NO_CONTENT)
}

/// List secrets in a scope.
///
/// Accepts both service token (return all in scope) and SVID (ABAC filtered).
async fn list_secrets(
    State(state): State<PgAppState>,
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

    let secrets = state.repository.list(&claims, scope).await;

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
/// Accepts both service token (return all) and SVID (ABAC filtered).
async fn list_service_secrets(
    State(state): State<PgAppState>,
    headers: HeaderMap,
    Path(service): Path<String>,
) -> impl IntoResponse {
    let claims = if validate_service_token(&headers, state.service_token.as_ref()).is_ok() {
        state.service_token_claims()
    } else {
        extract_svid(&headers, &state.svid_validator)?
    };

    let secrets = state.repository.list_service(&claims, &service).await;

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
/// Accepts both service token (return all) and SVID (ABAC filtered).
async fn list_instance_secrets(
    State(state): State<PgAppState>,
    headers: HeaderMap,
    Path(instance_id): Path<String>,
) -> impl IntoResponse {
    let claims = if validate_service_token(&headers, state.service_token.as_ref()).is_ok() {
        state.service_token_claims()
    } else {
        extract_svid(&headers, &state.svid_validator)?
    };

    let secrets = state.repository.list_instance(&claims, &instance_id).await;

    let response = ListSecretsResponse {
        secrets: secrets
            .into_iter()
            .map(SecretMetadataResponse::from)
            .collect(),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Query audit logs from `PostgreSQL`.
///
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
async fn get_audit_logs(
    State(state): State<PgAppState>,
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

    let db_query = AuditLogQuery {
        event_type: query.event_type.and_then(|s| s.parse().ok()),
        principal_id: query.principal_id,
        secret_name: query.secret_name,
        limit: query.limit.and_then(|l| i64::try_from(l).ok()),
        offset: query.offset.and_then(|o| i64::try_from(o).ok()),
    };

    let entries = audit_repo::list_audit_entries(state.repository.pool(), &db_query)
        .await
        .unwrap_or_default();

    let total = audit_repo::count_audit_entries(state.repository.pool(), &db_query)
        .await
        .unwrap_or(0);
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let total = total.max(0) as usize;

    let response = AuditLogsResponse {
        entries: entries
            .iter()
            .map(|e| AuditEntryResponse {
                id: e.id.to_string(),
                event_type: from_db_audit_event_type(e.event_type),
                principal_type: e.principal_type.map(from_db_principal_type),
                principal_id: e.principal_id.clone(),
                spiffe_id: e.spiffe_id.clone(),
                secret_scope: e.secret_scope.map(from_db_scope),
                secret_name: e.secret_name.clone(),
                timestamp: e.timestamp.to_rfc3339(),
            })
            .collect(),
        total,
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Rotate a secret's DEK.
///
/// Service token required. SVID tokens are denied with 403 FORBIDDEN.
async fn rotate_dek(
    State(state): State<PgAppState>,
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
        .rotate_dek(
            &claims,
            scope,
            service.as_deref(),
            instance_id.as_deref(),
            &name,
        )
        .await
        .map_err(|e| {
            if let crate::Error::SecretNotFound { .. } = e {
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

// =============================================================================
// Type conversions
// =============================================================================

const fn from_db_scope(scope: moto_keybox_db::Scope) -> Scope {
    match scope {
        moto_keybox_db::Scope::Global => Scope::Global,
        moto_keybox_db::Scope::Service => Scope::Service,
        moto_keybox_db::Scope::Instance => Scope::Instance,
    }
}

const fn from_db_principal_type(pt: moto_keybox_db::PrincipalType) -> PrincipalType {
    match pt {
        moto_keybox_db::PrincipalType::Garage => PrincipalType::Garage,
        moto_keybox_db::PrincipalType::Bike => PrincipalType::Bike,
        moto_keybox_db::PrincipalType::Service => PrincipalType::Service,
    }
}

const fn from_db_audit_event_type(et: moto_keybox_db::AuditEventType) -> AuditEventType {
    match et {
        moto_keybox_db::AuditEventType::Accessed => AuditEventType::Accessed,
        moto_keybox_db::AuditEventType::Created => AuditEventType::Created,
        moto_keybox_db::AuditEventType::Updated => AuditEventType::Updated,
        moto_keybox_db::AuditEventType::Deleted => AuditEventType::Deleted,
        moto_keybox_db::AuditEventType::SvidIssued => AuditEventType::SvidIssued,
        moto_keybox_db::AuditEventType::AuthFailed => AuditEventType::AuthFailed,
        moto_keybox_db::AuditEventType::AccessDenied => AuditEventType::AccessDenied,
        moto_keybox_db::AuditEventType::DekRotated => AuditEventType::DekRotated,
    }
}

// =============================================================================
// Router
// =============================================================================

/// Creates the PostgreSQL-backed keybox API router.
pub fn pg_router(state: PgAppState) -> Router {
    Router::new()
        .route("/auth/token", post(issue_token))
        .route("/auth/issue-garage-svid", post(issue_garage_svid))
        .route(
            "/secrets/{scope}/{name}",
            get(get_secret).post(set_secret).delete(delete_secret),
        )
        .route("/secrets/{scope}", get(list_secrets))
        .route("/secrets/service/{service}", get(list_service_secrets))
        .route(
            "/secrets/instance/{instance_id}",
            get(list_instance_secrets),
        )
        .route("/audit/logs", get(get_audit_logs))
        .route("/admin/rotate-dek/{name}", post(rotate_dek))
        .with_state(state)
}
