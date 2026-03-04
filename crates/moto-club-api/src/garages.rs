//! Garage REST endpoints.
//!
//! Provides endpoints for managing garages:
//! - `POST /api/v1/garages` - Create a garage
//! - `GET /api/v1/garages` - List garages (filtered by owner)
//! - `GET /api/v1/garages/{name}` - Get garage details
//! - `DELETE /api/v1/garages/{name}` - Close/delete garage
//! - `POST /api/v1/garages/{name}/extend` - Extend TTL

// Match is more readable than if-let for error handling with wildcards
#![allow(clippy::single_match_else)]

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiError, AppState, error_codes};
use moto_club_db::{DbError, Garage, GarageStatus, TerminationReason, garage_repo};
use moto_club_garage::{
    CreateGarageInput, DEFAULT_TTL_SECONDS, GarageServiceError, MAX_TTL_SECONDS, MIN_TTL_SECONDS,
};
use moto_club_k8s::GarageNamespaceOps;
use moto_club_types::GarageId;

/// Request to create a new garage.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateGarageRequest {
    /// Human-friendly name (auto-generated if omitted).
    pub name: Option<String>,
    /// Git branch (CLI determines from local repo if omitted).
    pub branch: Option<String>,
    /// Time-to-live in seconds (default: 14400 = 4h).
    pub ttl_seconds: Option<i32>,
    /// Override dev container image.
    pub image: Option<String>,
    /// Include `PostgreSQL` supporting service (postgres:16).
    #[serde(default)]
    pub with_postgres: bool,
    /// Include Redis supporting service (redis:7).
    #[serde(default)]
    pub with_redis: bool,
}

/// Request to extend a garage's TTL.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtendTtlRequest {
    /// Seconds to add to current expiry.
    pub seconds: i32,
}

/// Response for extending a garage's TTL.
///
/// Per spec (moto-club.md lines 379-386), returns just the new expiry info.
#[derive(Debug, Clone, Serialize)]
pub struct ExtendTtlResponse {
    /// When the garage expires after extension.
    pub expires_at: DateTime<Utc>,
    /// Seconds remaining until expiry.
    pub ttl_remaining_seconds: i64,
}

/// Query parameters for listing garages.
#[derive(Debug, Clone, Deserialize)]
pub struct ListGaragesQuery {
    /// Filter by status (comma-separated). Valid: pending, initializing, ready, failed, terminated.
    pub status: Option<String>,
    /// Include terminated garages (default: false).
    #[serde(default)]
    pub all: bool,
}

/// Response for a garage.
#[derive(Debug, Clone, Serialize)]
pub struct GarageResponse {
    /// Unique identifier.
    pub id: Uuid,
    /// Human-friendly name.
    pub name: String,
    /// Owner identifier.
    pub owner: String,
    /// Git branch.
    pub branch: String,
    /// Current status.
    pub status: GarageStatus,
    /// Dev container image used.
    pub image: String,
    /// Time-to-live in seconds.
    pub ttl_seconds: i32,
    /// When the garage expires.
    pub expires_at: DateTime<Utc>,
    /// Kubernetes namespace.
    pub namespace: String,
    /// Kubernetes pod name.
    pub pod_name: String,
    /// When the garage was created.
    pub created_at: DateTime<Utc>,
    /// When the garage was last updated.
    pub updated_at: DateTime<Utc>,
    /// When the garage was terminated (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminated_at: Option<DateTime<Utc>>,
    /// Why the garage was terminated (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub termination_reason: Option<TerminationReason>,
}

impl From<Garage> for GarageResponse {
    fn from(g: Garage) -> Self {
        Self {
            id: g.id,
            name: g.name,
            owner: g.owner,
            branch: g.branch,
            status: g.status,
            image: g.image,
            ttl_seconds: g.ttl_seconds,
            expires_at: g.expires_at,
            namespace: g.namespace,
            pod_name: g.pod_name,
            created_at: g.created_at,
            updated_at: g.updated_at,
            terminated_at: g.terminated_at,
            termination_reason: g.termination_reason,
        }
    }
}

/// Response for listing garages.
#[derive(Debug, Clone, Serialize)]
pub struct ListGaragesResponse {
    /// List of garages.
    pub garages: Vec<GarageResponse>,
}

/// Extract owner from Authorization header.
///
/// For local dev, the Bearer token IS the username.
/// e.g., "Authorization: Bearer nick" means owner = "nick"
fn extract_owner(headers: &HeaderMap) -> Result<String, (StatusCode, Json<ApiError>)> {
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

    // Extract Bearer token
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

/// Generate a random garage name (adjective-animal format).
fn generate_garage_name() -> String {
    use rand::prelude::IndexedRandom;

    let adjectives = [
        "bold", "swift", "quick", "bright", "calm", "cool", "dark", "fast", "gold", "green",
        "happy", "keen", "kind", "loud", "mild", "neat", "nice", "old", "pink", "pure", "rare",
        "red", "rich", "safe", "shy", "slim", "soft", "tall", "tiny", "warm", "wild", "wise",
    ];
    let animals = [
        "ant", "bat", "bear", "bird", "bull", "cat", "crab", "crow", "deer", "dog", "dove", "duck",
        "eagle", "eel", "elk", "fish", "fox", "frog", "goat", "hawk", "lion", "lynx", "mole",
        "moth", "mouse", "newt", "owl", "panda", "pig", "rat", "seal", "shark", "slug", "snake",
        "swan", "tiger", "toad", "wasp", "wolf", "wren",
    ];

    let mut rng = rand::rng();
    let adj = adjectives.choose(&mut rng).unwrap_or(&"bold");
    let animal = animals.choose(&mut rng).unwrap_or(&"mongoose");

    format!("{adj}-{animal}")
}

/// Create a new garage.
///
/// POST /api/v1/garages
///
/// When `GarageService` is configured, this creates the full K8s resources:
/// - K8s namespace with labels
/// - Dev container pod (with ttyd terminal daemon)
///
/// When `GarageService` is not configured (testing/local dev without K8s),
/// only the database record is created.
#[allow(clippy::too_many_lines)]
async fn create_garage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateGarageRequest>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // If GarageService is available, use it for full K8s integration
    if let Some(ref garage_service) = state.garage_service {
        let input = CreateGarageInput {
            name: req.name,
            branch: req.branch.unwrap_or_else(|| "main".to_string()),
            ttl_seconds: req.ttl_seconds,
            image: req.image,
            engine: None,
            repo: None, // TODO: Add repo to API request when needed
            with_postgres: req.with_postgres,
            with_redis: req.with_redis,
        };

        let garage = garage_service
            .create(&owner, input)
            .await
            .map_err(map_garage_service_error)?;

        return Ok((StatusCode::CREATED, Json(GarageResponse::from(garage))));
    }

    // Fallback: database-only creation (no K8s resources)
    // This path is used when K8s is not configured (testing, local dev)
    tracing::warn!(
        "GarageService not configured, creating database record only (no K8s resources)"
    );

    // Validate TTL
    let ttl_seconds = req.ttl_seconds.unwrap_or(*DEFAULT_TTL_SECONDS);
    if ttl_seconds < *MIN_TTL_SECONDS || ttl_seconds > *MAX_TTL_SECONDS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                error_codes::INVALID_TTL,
                format!(
                    "TTL must be between {} and {} seconds",
                    *MIN_TTL_SECONDS, *MAX_TTL_SECONDS
                ),
            )),
        ));
    }

    // Generate name if not provided
    let name = req.name.unwrap_or_else(generate_garage_name);

    // Validate name format (lowercase alphanumeric with hyphens)
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_NAME",
                "Garage name must contain only lowercase letters, numbers, and hyphens",
            )),
        ));
    }

    // Generate ID
    let id = Uuid::now_v7();
    let garage_id = GarageId::from_uuid(id);
    let namespace = format!("moto-garage-{}", garage_id.short());
    let pod_name = "dev-container".to_string();
    let branch = req.branch.unwrap_or_else(|| "main".to_string());
    // Default image can be overridden via request; falls back to env var or hardcoded default
    let image = req.image.unwrap_or_else(|| {
        state.garage_k8s.as_ref().map_or_else(
            || moto_club_garage::DEFAULT_IMAGE.to_string(),
            |k8s| k8s.dev_container_image().to_string(),
        )
    });

    // Create in database
    let input = garage_repo::CreateGarage {
        id,
        name: name.clone(),
        owner,
        branch,
        image,
        ttl_seconds,
        namespace,
        pod_name,
    };

    let garage = garage_repo::create(&state.db_pool, input)
        .await
        .map_err(|e| {
            match e {
                DbError::AlreadyExists { .. } => (
                    StatusCode::CONFLICT,
                    Json(ApiError::new(
                        error_codes::GARAGE_ALREADY_EXISTS,
                        format!("Garage name '{name}' is already taken"),
                    )),
                ),
                DbError::Sqlx(e) => {
                    tracing::error!("Database error creating garage: {e}");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
                    )
                }
                DbError::NotFound { .. } | DbError::NotOwned { .. } | DbError::Migration(_) => {
                    // Shouldn't happen for create
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ApiError::new(
                            error_codes::INTERNAL_ERROR,
                            "Unexpected error",
                        )),
                    )
                }
            }
        })?;

    Ok((StatusCode::CREATED, Json(GarageResponse::from(garage))))
}

/// Maps `GarageServiceError` to HTTP response.
fn map_garage_service_error(e: GarageServiceError) -> (StatusCode, Json<ApiError>) {
    match e {
        GarageServiceError::AlreadyExists(name) => (
            StatusCode::CONFLICT,
            Json(ApiError::new(
                error_codes::GARAGE_ALREADY_EXISTS,
                format!("Garage name '{name}' is already taken"),
            )),
        ),
        GarageServiceError::NotFound(name) => (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                error_codes::GARAGE_NOT_FOUND,
                format!("Garage '{name}' not found"),
            )),
        ),
        GarageServiceError::NotOwned { name, .. } => (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::GARAGE_NOT_OWNED,
                format!("Garage '{name}' exists but is owned by another user"),
            )),
        ),
        GarageServiceError::Terminated(name) => (
            StatusCode::GONE,
            Json(ApiError::new(
                error_codes::GARAGE_TERMINATED,
                format!("Garage '{name}' has been terminated"),
            )),
        ),
        GarageServiceError::Expired(name) => (
            StatusCode::GONE,
            Json(ApiError::new(
                error_codes::GARAGE_EXPIRED,
                format!("Garage '{name}' has expired"),
            )),
        ),
        GarageServiceError::InvalidTtl { message } => (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(error_codes::INVALID_TTL, message)),
        ),
        GarageServiceError::NameGenerationFailed { attempts } => {
            tracing::error!(attempts, "failed to generate unique garage name");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "Failed to generate unique garage name",
                )),
            )
        }
        GarageServiceError::Database(e) => {
            tracing::error!("Database error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
            )
        }
        GarageServiceError::Kubernetes(e) => {
            tracing::error!("Kubernetes error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::K8S_ERROR,
                    "Kubernetes operation failed",
                )),
            )
        }
        GarageServiceError::Lifecycle(e) => {
            tracing::error!("Lifecycle error: {e}");
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(error_codes::INTERNAL_ERROR, e.to_string())),
            )
        }
        GarageServiceError::Keybox(e) => {
            tracing::error!("Keybox error: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "Keybox operation failed",
                )),
            )
        }
    }
}

/// Parse a status string into `GarageStatus`.
fn parse_status(s: &str) -> Option<GarageStatus> {
    match s.trim().to_lowercase().as_str() {
        "pending" => Some(GarageStatus::Pending),
        "initializing" => Some(GarageStatus::Initializing),
        "ready" => Some(GarageStatus::Ready),
        "failed" => Some(GarageStatus::Failed),
        "terminated" => Some(GarageStatus::Terminated),
        _ => None,
    }
}

/// List garages for the authenticated owner.
///
/// GET /api/v1/garages
///
/// Query parameters per spec lines 295-300:
/// - `?status=running,ready` - filter by status (comma-separated)
/// - `?all=true` - include terminated garages (default: false)
async fn list_garages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListGaragesQuery>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // Parse status filter if provided
    let status_filter: Option<Vec<GarageStatus>> = if let Some(ref status_str) = query.status {
        let mut statuses = Vec::new();
        for s in status_str.split(',') {
            match parse_status(s) {
                Some(status) => statuses.push(status),
                None => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ApiError::new(
                            error_codes::INVALID_STATUS,
                            format!("Unknown status value: '{}'", s.trim()),
                        )),
                    ));
                }
            }
        }
        Some(statuses)
    } else {
        None
    };

    let garages = garage_repo::list_by_owner(&state.db_pool, &owner, query.all)
        .await
        .map_err(|e| {
            tracing::error!("Database error listing garages: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
            )
        })?;

    // Apply status filter if provided
    let filtered_garages: Vec<Garage> = if let Some(statuses) = status_filter {
        garages
            .into_iter()
            .filter(|g| statuses.contains(&g.status))
            .collect()
    } else {
        garages
    };

    let response = ListGaragesResponse {
        garages: filtered_garages
            .into_iter()
            .map(GarageResponse::from)
            .collect(),
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Get a garage by name.
///
/// GET /api/v1/garages/{name}
async fn get_garage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    let garage = garage_repo::get_by_name(&state.db_pool, &name)
        .await
        .map_err(|e| match e {
            DbError::NotFound { .. } => (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::GARAGE_NOT_FOUND,
                    format!("Garage '{name}' not found"),
                )),
            ),
            DbError::Sqlx(e) => {
                tracing::error!("Database error getting garage: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
                )
            }
            DbError::AlreadyExists { .. } | DbError::NotOwned { .. } | DbError::Migration(_) => {
                // Shouldn't happen for get
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(
                        error_codes::INTERNAL_ERROR,
                        "Unexpected error",
                    )),
                )
            }
        })?;

    // Check ownership
    if garage.owner != owner {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::GARAGE_NOT_OWNED,
                format!("Garage '{name}' exists but is owned by another user"),
            )),
        ));
    }

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(GarageResponse::from(garage))))
}

/// Delete (close) a garage.
///
/// DELETE /api/v1/garages/{name}
///
/// Close flow (per spec lines 903-907):
/// 1. Update database status to Terminated
/// 2. Set `terminated_at` timestamp
/// 3. Set `termination_reason`
/// 4. Delete K8s namespace (cascades to all resources)
///
/// Idempotent: deleting already-terminated garage returns 204.
async fn delete_garage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    let garage = garage_repo::get_by_name(&state.db_pool, &name)
        .await
        .map_err(|e| match e {
            DbError::NotFound { .. } => (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::GARAGE_NOT_FOUND,
                    format!("Garage '{name}' not found"),
                )),
            ),
            _ => {
                tracing::error!("Database error getting garage: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
                )
            }
        })?;

    // Check ownership
    if garage.owner != owner {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::GARAGE_NOT_OWNED,
                format!("Garage '{name}' exists but is owned by another user"),
            )),
        ));
    }

    // Idempotent: if already terminated, return 204 success
    if garage.status == GarageStatus::Terminated {
        return Ok(StatusCode::NO_CONTENT);
    }

    // Terminate the garage in database first (source of truth for "user requested close")
    garage_repo::terminate(&state.db_pool, garage.id, TerminationReason::UserClosed)
        .await
        .map_err(|e| {
            tracing::error!("Database error terminating garage: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
            )
        })?;

    // Close all active sessions and broadcast peer removals
    let garage_id_str = garage.id.to_string();
    match state.session_manager.on_garage_terminated(&garage_id_str) {
        Ok(closed_sessions) => {
            for session in &closed_sessions {
                state
                    .peer_broadcaster
                    .broadcast_remove(&session.garage_id, session.device_pubkey.clone());
            }
            if !closed_sessions.is_empty() {
                tracing::info!(
                    garage_id = %garage.id,
                    garage_name = %name,
                    sessions_closed = closed_sessions.len(),
                    "closed active sessions on garage termination"
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                garage_id = %garage.id,
                garage_name = %name,
                error = %e,
                "failed to close sessions on garage termination"
            );
            // Don't fail the delete — garage is already terminated in DB
        }
    }

    // Delete K8s namespace (cascades to all resources)
    // Per spec: if deletion fails, log error and return success anyway
    // Reconciler will retry namespace deletion on next cycle
    if let Some(ref garage_k8s) = state.garage_k8s {
        let garage_id = GarageId::from_uuid(garage.id);
        if let Err(e) = garage_k8s.delete_garage_namespace(&garage_id).await {
            tracing::warn!(
                garage_id = %garage.id,
                garage_name = %name,
                error = %e,
                "failed to delete K8s namespace (may already be deleted)"
            );
            // Don't fail - user intent captured, reconciler will clean up
        }
    }

    Ok::<_, (StatusCode, Json<ApiError>)>(StatusCode::NO_CONTENT)
}

/// Extend a garage's TTL.
///
/// POST /api/v1/garages/{name}/extend
async fn extend_garage_ttl(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
    Json(req): Json<ExtendTtlRequest>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    let garage = garage_repo::get_by_name(&state.db_pool, &name)
        .await
        .map_err(|e| match e {
            DbError::NotFound { .. } => (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::GARAGE_NOT_FOUND,
                    format!("Garage '{name}' not found"),
                )),
            ),
            _ => {
                tracing::error!("Database error getting garage: {e}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
                )
            }
        })?;

    // Check ownership
    if garage.owner != owner {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::GARAGE_NOT_OWNED,
                format!("Garage '{name}' exists but is owned by another user"),
            )),
        ));
    }

    // Check if terminated
    if garage.status == GarageStatus::Terminated {
        return Err((
            StatusCode::GONE,
            Json(ApiError::new(
                error_codes::GARAGE_TERMINATED,
                format!("Garage '{name}' has been terminated"),
            )),
        ));
    }

    // Check if expired
    if garage.expires_at < Utc::now() {
        return Err((
            StatusCode::GONE,
            Json(ApiError::new(
                error_codes::GARAGE_EXPIRED,
                format!("Garage '{name}' has expired and cannot be extended"),
            )),
        ));
    }

    // Validate extension amount
    if req.seconds <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                error_codes::INVALID_TTL,
                "Extension seconds must be positive",
            )),
        ));
    }

    // Check if total TTL would exceed max
    let new_ttl = garage.ttl_seconds + req.seconds;
    if new_ttl > *MAX_TTL_SECONDS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                error_codes::INVALID_TTL,
                format!(
                    "Total TTL would be {new_ttl}s, which exceeds maximum of {}s",
                    *MAX_TTL_SECONDS
                ),
            )),
        ));
    }

    // Extend TTL
    let garage = garage_repo::extend_ttl(&state.db_pool, garage.id, req.seconds)
        .await
        .map_err(|e| {
            tracing::error!("Database error extending TTL: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
            )
        })?;

    // Calculate remaining TTL in seconds
    let now = Utc::now();
    let ttl_remaining_seconds = (garage.expires_at - now).num_seconds().max(0);

    let response = ExtendTtlResponse {
        expires_at: garage.expires_at,
        ttl_remaining_seconds,
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Creates the garage router.
///
/// Includes:
/// - `POST /api/v1/garages` - Create a garage
/// - `GET /api/v1/garages` - List garages
/// - `GET /api/v1/garages/{name}` - Get garage details
/// - `DELETE /api/v1/garages/{name}` - Close/delete garage
/// - `POST /api/v1/garages/{name}/extend` - Extend TTL
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/garages", post(create_garage).get(list_garages))
        .route(
            "/api/v1/garages/{name}",
            get(get_garage).delete(delete_garage),
        )
        .route("/api/v1/garages/{name}/extend", post(extend_garage_ttl))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_garage_request_deserialize() {
        let json = r#"{"name": "my-garage", "ttl_seconds": 7200}"#;
        let req: CreateGarageRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name, Some("my-garage".to_string()));
        assert_eq!(req.ttl_seconds, Some(7200));
        assert!(req.branch.is_none());
        assert!(req.image.is_none());
        assert!(!req.with_postgres);
        assert!(!req.with_redis);
    }

    #[test]
    fn create_garage_request_optional_fields() {
        let json = r"{}";
        let req: CreateGarageRequest = serde_json::from_str(json).unwrap();
        assert!(req.name.is_none());
        assert!(req.branch.is_none());
        assert!(req.ttl_seconds.is_none());
        assert!(req.image.is_none());
        assert!(!req.with_postgres);
        assert!(!req.with_redis);
    }

    #[test]
    fn create_garage_request_with_services() {
        let json = r#"{"with_postgres": true, "with_redis": true}"#;
        let req: CreateGarageRequest = serde_json::from_str(json).unwrap();
        assert!(req.with_postgres);
        assert!(req.with_redis);
    }

    #[test]
    fn extend_ttl_request_deserialize() {
        let json = r#"{"seconds": 3600}"#;
        let req: ExtendTtlRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.seconds, 3600);
    }

    #[test]
    fn extend_ttl_response_serialization() {
        use chrono::TimeZone;

        let response = ExtendTtlResponse {
            expires_at: Utc.with_ymd_and_hms(2026, 1, 28, 22, 0, 0).unwrap(),
            ttl_remaining_seconds: 21600,
        };

        let json = serde_json::to_string(&response).unwrap();
        // Should only contain expires_at and ttl_remaining_seconds per spec
        assert!(json.contains(r#""expires_at":"2026-01-28T22:00:00Z""#));
        assert!(json.contains(r#""ttl_remaining_seconds":21600"#));
        // Should NOT contain garage details
        assert!(!json.contains("name"));
        assert!(!json.contains("owner"));
        assert!(!json.contains("status"));
    }

    #[test]
    fn garage_response_serialization() {
        let garage = Garage {
            id: Uuid::nil(),
            name: "bold-mongoose".to_string(),
            owner: "nick".to_string(),
            branch: "main".to_string(),
            image: "ghcr.io/example/dev:latest".to_string(),
            status: GarageStatus::Ready,
            ttl_seconds: 14400,
            expires_at: Utc::now(),
            namespace: "moto-garage-abc123".to_string(),
            pod_name: "dev-container".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            terminated_at: None,
            termination_reason: None,
        };

        let response = GarageResponse::from(garage);
        let json = serde_json::to_string(&response).unwrap();

        assert!(json.contains(r#""name":"bold-mongoose""#));
        assert!(json.contains(r#""owner":"nick""#));
        assert!(json.contains(r#""status":"ready""#));
        assert!(json.contains("updated_at"));
        // terminated_at and termination_reason should not be present when None
        assert!(!json.contains("terminated_at"));
        assert!(!json.contains("termination_reason"));
    }

    #[test]
    fn garage_response_with_termination() {
        let garage = Garage {
            id: Uuid::nil(),
            name: "old-garage".to_string(),
            owner: "nick".to_string(),
            branch: "main".to_string(),
            image: "ghcr.io/example/dev:latest".to_string(),
            status: GarageStatus::Terminated,
            ttl_seconds: 14400,
            expires_at: Utc::now(),
            namespace: "moto-garage-xyz".to_string(),
            pod_name: "dev-container".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            terminated_at: Some(Utc::now()),
            termination_reason: Some(TerminationReason::UserClosed),
        };

        let response = GarageResponse::from(garage);
        let json = serde_json::to_string(&response).unwrap();

        assert!(json.contains(r#""status":"terminated""#));
        assert!(json.contains("terminated_at"));
        assert!(json.contains(r#""termination_reason":"user_closed""#));
    }

    #[test]
    fn list_garages_query_defaults() {
        let query: ListGaragesQuery = serde_json::from_str("{}").unwrap();
        assert!(!query.all);
        assert!(query.status.is_none());
    }

    #[test]
    fn list_garages_query_with_status() {
        let query: ListGaragesQuery =
            serde_json::from_str(r#"{"status": "initializing,ready", "all": true}"#).unwrap();
        assert!(query.all);
        assert_eq!(query.status, Some("initializing,ready".to_string()));
    }

    #[test]
    fn parse_status_valid() {
        assert_eq!(parse_status("pending"), Some(GarageStatus::Pending));
        assert_eq!(
            parse_status("initializing"),
            Some(GarageStatus::Initializing)
        );
        assert_eq!(parse_status("ready"), Some(GarageStatus::Ready));
        assert_eq!(parse_status("failed"), Some(GarageStatus::Failed));
        assert_eq!(parse_status("terminated"), Some(GarageStatus::Terminated));
        // Case insensitive
        assert_eq!(
            parse_status("INITIALIZING"),
            Some(GarageStatus::Initializing)
        );
        assert_eq!(
            parse_status(" initializing "),
            Some(GarageStatus::Initializing)
        );
    }

    #[test]
    fn parse_status_invalid() {
        assert_eq!(parse_status("invalid"), None);
        assert_eq!(parse_status(""), None);
    }

    #[test]
    fn generated_garage_name_format() {
        let name = generate_garage_name();
        assert!(name.contains('-'));
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
    }
}
