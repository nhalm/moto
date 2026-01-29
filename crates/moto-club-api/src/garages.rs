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
use moto_club_k8s::GarageNamespaceOps;
use moto_club_types::GarageId;

/// Default TTL in seconds (4 hours).
const DEFAULT_TTL_SECONDS: i32 = 14400;

/// Maximum TTL in seconds (48 hours).
const MAX_TTL_SECONDS: i32 = 172_800;

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
}

/// Request to extend a garage's TTL.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtendTtlRequest {
    /// Seconds to add to current expiry.
    pub seconds: i32,
}

/// Query parameters for listing garages.
#[derive(Debug, Clone, Deserialize)]
pub struct ListGaragesQuery {
    /// Include terminated garages (default: false).
    #[serde(default)]
    pub include_terminated: bool,
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
async fn create_garage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateGarageRequest>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // Validate TTL
    let ttl_seconds = req.ttl_seconds.unwrap_or(DEFAULT_TTL_SECONDS);
    if ttl_seconds <= 0 || ttl_seconds > MAX_TTL_SECONDS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                error_codes::INVALID_TTL,
                format!("TTL must be between 1 and {MAX_TTL_SECONDS} seconds"),
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
    let namespace = format!("moto-garage-{id}");
    let pod_name = "dev-container".to_string();
    let branch = req.branch.unwrap_or_else(|| "main".to_string());
    // Default image can be overridden via request
    let image = req
        .image
        .unwrap_or_else(|| "ghcr.io/nhalm/moto-dev:latest".to_string());

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
                DbError::NotFound { .. } | DbError::NotOwned { .. } => {
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

    // TODO: Create K8s namespace and deploy pod (moto-club-k8s crate)
    // For now, just return the database record

    Ok((StatusCode::CREATED, Json(GarageResponse::from(garage))))
}

/// List garages for the authenticated owner.
///
/// GET /api/v1/garages
async fn list_garages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListGaragesQuery>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    let garages = garage_repo::list_by_owner(&state.db_pool, &owner, query.include_terminated)
        .await
        .map_err(|e| {
            tracing::error!("Database error listing garages: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(error_codes::DATABASE_ERROR, "Database error")),
            )
        })?;

    let response = ListGaragesResponse {
        garages: garages.into_iter().map(GarageResponse::from).collect(),
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
            DbError::AlreadyExists { .. } | DbError::NotOwned { .. } => {
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
/// 2. Set terminated_at timestamp
/// 3. Set termination_reason
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
    if new_ttl > MAX_TTL_SECONDS {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                error_codes::INVALID_TTL,
                format!(
                    "Total TTL would be {new_ttl}s, which exceeds maximum of {MAX_TTL_SECONDS}s"
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

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(GarageResponse::from(garage))))
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
    }

    #[test]
    fn create_garage_request_optional_fields() {
        let json = r#"{}"#;
        let req: CreateGarageRequest = serde_json::from_str(json).unwrap();
        assert!(req.name.is_none());
        assert!(req.branch.is_none());
        assert!(req.ttl_seconds.is_none());
        assert!(req.image.is_none());
    }

    #[test]
    fn extend_ttl_request_deserialize() {
        let json = r#"{"seconds": 3600}"#;
        let req: ExtendTtlRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.seconds, 3600);
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
        assert!(!query.include_terminated);
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
