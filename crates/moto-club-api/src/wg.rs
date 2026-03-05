//! `WireGuard` coordination REST endpoints.
//!
//! Provides endpoints for `WireGuard` tunnel coordination:
//! - `POST /api/v1/wg/devices` - Register client device
//! - `GET /api/v1/wg/devices/{id}` - Get device info
//! - `POST /api/v1/wg/sessions` - Create tunnel session
//! - `GET /api/v1/wg/sessions` - List active sessions
//! - `DELETE /api/v1/wg/sessions/{id}` - Close session
//! - `POST /api/v1/wg/garages` - Register garage (called by garage pod)
//! - `GET /api/v1/wg/garages/{id}/peers` - Get peer list (garage polls this)
//! - `GET /api/v1/wg/derp-map` - Get DERP server map
//! - `WS /internal/wg/garages/{id}/peers` - WebSocket for real-time peer streaming

use std::net::SocketAddr;

use axum::{
    Json, Router,
    extract::{Path, Query, State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use chrono::{DateTime, Utc};
use moto_club_wg::{
    CreateSessionRequest as WgCreateSessionRequest, CreateSessionResponse, DeviceRegistration,
    GarageConnectionInfo, GarageRegistration, RegisteredDevice, Session,
};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiError, AppState, error_codes};
use moto_club_db::{GarageStatus, garage_repo};
use moto_k8s::TokenReviewOps;

// ============================================================================
// Request/Response types
// ============================================================================

/// Request to register a client device.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterDeviceRequest {
    /// Device's `WireGuard` public key.
    pub public_key: WgPublicKey,
    /// Optional human-readable device name (e.g., "macbook-pro").
    pub device_name: Option<String>,
}

/// Response for device registration.
///
/// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceResponse {
    /// Device's `WireGuard` public key (IS the device identity).
    pub public_key: WgPublicKey,
    /// Assigned overlay IP address.
    #[serde(rename = "assigned_ip")]
    pub overlay_ip: OverlayIp,
    /// Optional human-readable device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// When the device was registered.
    pub created_at: DateTime<Utc>,
}

impl From<RegisteredDevice> for DeviceResponse {
    fn from(d: RegisteredDevice) -> Self {
        Self {
            public_key: d.public_key,
            overlay_ip: d.overlay_ip,
            device_name: d.device_name,
            created_at: d.created_at,
        }
    }
}

/// Request to create a tunnel session.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateSessionRequest {
    /// Garage to connect to (name or ID).
    pub garage_id: Uuid,
    /// Device requesting the connection (`WireGuard` public key IS the device identity).
    pub device_pubkey: WgPublicKey,
    /// Optional session TTL in seconds. Defaults to garage TTL or 4 hours.
    pub ttl_seconds: Option<u32>,
}

/// Response for session creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    /// Unique session identifier (prefixed with "sess_").
    pub session_id: String,
    /// Garage connection info.
    pub garage: GarageConnectionInfo,
    /// Client's overlay IP address.
    pub client_ip: OverlayIp,
    /// DERP relay map for fallback connections.
    pub derp_map: DerpMap,
    /// When this session expires.
    pub expires_at: DateTime<Utc>,
}

impl From<CreateSessionResponse> for SessionResponse {
    fn from(r: CreateSessionResponse) -> Self {
        Self {
            session_id: r.session_id,
            garage: r.garage,
            client_ip: r.client_ip,
            derp_map: r.derp_map,
            expires_at: r.expires_at,
        }
    }
}

/// Query parameters for listing sessions.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ListSessionsQuery {
    /// Filter by garage ID (optional).
    pub garage_id: Option<Uuid>,
    /// Include expired/closed sessions (default: false).
    #[serde(default)]
    pub all: bool,
}

/// Response for listing sessions.
#[derive(Debug, Clone, Serialize)]
pub struct ListSessionsResponse {
    /// Active sessions.
    pub sessions: Vec<SessionInfo>,
}

/// Session info for listing.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    /// Unique session identifier.
    pub session_id: String,
    /// Garage this session connects to.
    pub garage_id: String,
    /// Human-readable garage name.
    pub garage_name: String,
    /// Device public key (`WireGuard` public key IS the device identity).
    pub device_pubkey: WgPublicKey,
    /// Optional device name for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// When this session was created.
    pub created_at: DateTime<Utc>,
    /// When this session expires.
    pub expires_at: DateTime<Utc>,
}

impl From<Session> for SessionInfo {
    fn from(s: Session) -> Self {
        Self {
            session_id: s.session_id,
            garage_id: s.garage_id,
            garage_name: s.garage_name,
            device_pubkey: s.device_pubkey,
            device_name: None, // Would need to look up from device registry
            created_at: s.created_at,
            expires_at: s.expires_at,
        }
    }
}

/// Request to register a garage (called by garage pod).
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterGarageRequest {
    /// Garage identifier (UUID or name).
    pub garage_id: String,
    /// Garage's ephemeral `WireGuard` public key.
    pub public_key: WgPublicKey,
    /// Direct UDP endpoints for P2P connections.
    #[serde(default)]
    pub endpoints: Vec<SocketAddr>,
}

/// Response for garage registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GarageWgResponse {
    /// Assigned overlay IP address.
    pub assigned_ip: OverlayIp,
    /// DERP relay map for fallback connections.
    pub derp_map: DerpMap,
}

/// Response for getting garage `WireGuard` registration.
///
/// GET /`api/v1/wg/garages/{garage_id`}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GarageWgRegistrationResponse {
    /// Garage identifier.
    pub garage_id: String,
    /// Garage's `WireGuard` public key.
    pub public_key: WgPublicKey,
    /// Garage's overlay IP address.
    pub assigned_ip: OverlayIp,
    /// Direct UDP endpoints for P2P connections.
    pub endpoints: Vec<SocketAddr>,
    /// Peer version counter (incremented on session create/close).
    pub peer_version: i32,
    /// DERP relay map for fallback connections.
    pub derp_map: DerpMap,
    /// When the garage registered.
    pub registered_at: DateTime<Utc>,
}

/// Response for peer list (garage polling).
#[derive(Debug, Clone, Serialize)]
pub struct PeerListResponse {
    /// Peers authorized to connect to this garage.
    pub peers: Vec<PeerInfo>,
    /// Version counter (incremented on session create/close).
    /// Used for conditional GET with `?version=N` query param.
    pub version: i32,
}

/// Information about an authorized peer.
#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    /// Peer's `WireGuard` public key.
    pub public_key: WgPublicKey,
    /// Peer's allowed IP (their overlay IP).
    pub allowed_ip: String,
}

/// Query parameters for `GET /api/v1/wg/garages/{id}/peers`.
///
/// Supports conditional GET with `?version=N` query parameter.
/// If the current version equals N, returns 304 Not Modified.
#[derive(Debug, Clone, Deserialize)]
pub struct GetPeersParams {
    /// Optional version for conditional GET.
    /// If provided and matches current version, returns 304 Not Modified.
    pub version: Option<i32>,
}

/// Response for DERP map endpoint.
///
/// GET /api/v1/wg/derp-map
///
/// Note: Only Serialize is derived because `#[serde(flatten)]` with `DerpMap`'s
/// `HashMap`<u16, _> regions field causes deserialization issues due to JSON
/// object keys being strings. This type is only used for API responses.
#[derive(Debug, Clone, Serialize)]
pub struct DerpMapResponse {
    /// DERP regions with their nodes.
    #[serde(flatten)]
    pub derp_map: DerpMap,
    /// Version number, incremented when DERP config changes.
    pub version: u32,
}

// ============================================================================
// Helper functions
// ============================================================================

/// Extract owner from Authorization header.
///
/// For local dev, the Bearer token IS the username.
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

/// Extract Bearer token from Authorization header (without validation).
fn extract_bearer_token(headers: &HeaderMap) -> Result<String, (StatusCode, Json<ApiError>)> {
    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError::new(
                    error_codes::INVALID_TOKEN,
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
                    error_codes::INVALID_TOKEN,
                    "Invalid Authorization header format, expected 'Bearer <token>'",
                )),
            )
        })?;

    if token.is_empty() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::INVALID_TOKEN,
                "Empty Bearer token",
            )),
        ));
    }

    Ok(token.to_string())
}

/// Validates K8s `ServiceAccount` token and verifies the pod is in the expected namespace.
///
/// Per spec (lines 585-603):
/// 1. K8s `ServiceAccount` token validated via `TokenReview` API
/// 2. Pod must be in namespace `moto-garage-{garage_id}`
/// 3. Prevents rogue pods from registering as arbitrary garages
///
/// # Arguments
///
/// * `state` - Application state containing the K8s client
/// * `headers` - HTTP headers containing the Authorization header
/// * `garage_id` - Expected garage ID (namespace must be `moto-garage-{garage_id}`)
///
/// # Returns
///
/// Returns `Ok(())` if validation passes, or an appropriate error if:
/// - No K8s client configured (skips validation in test/local dev mode)
/// - Token is invalid or expired (returns `INVALID_TOKEN`)
/// - Token namespace doesn't match expected garage namespace (returns `NAMESPACE_MISMATCH`)
async fn validate_garage_token(
    state: &AppState,
    headers: &HeaderMap,
    garage_id: &str,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    // If no K8s client configured, skip validation (test/local dev mode)
    let Some(k8s_client) = &state.k8s_client else {
        tracing::debug!(garage_id = %garage_id, "K8s token validation skipped (no client configured)");
        return Ok(());
    };

    // Extract the bearer token
    let token = extract_bearer_token(headers)?;

    // Validate the token via K8s TokenReview API
    let validated = k8s_client.validate_token(&token).await.map_err(|e| {
        tracing::warn!(garage_id = %garage_id, error = %e, "K8s token validation failed");
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiError::new(
                error_codes::INVALID_TOKEN,
                "K8s ServiceAccount token invalid or expired",
            )),
        )
    })?;

    // Expected namespace format: moto-garage-{short_id} (first 8 chars of garage UUID)
    let short_id = &garage_id[..8.min(garage_id.len())];
    let expected_namespace = format!("moto-garage-{short_id}");

    // Check that the service account is in the correct namespace
    let actual_namespace = validated.service_account_namespace().ok_or_else(|| {
        tracing::warn!(
            garage_id = %garage_id,
            username = %validated.username,
            "Token doesn't belong to a service account"
        );
        (
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::NAMESPACE_MISMATCH,
                "Token must belong to a K8s ServiceAccount",
            )),
        )
    })?;

    if actual_namespace != expected_namespace {
        tracing::warn!(
            garage_id = %garage_id,
            expected = %expected_namespace,
            actual = %actual_namespace,
            "Namespace mismatch"
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::NAMESPACE_MISMATCH,
                format!(
                    "Pod not running in expected namespace: expected '{expected_namespace}', got '{actual_namespace}'"
                ),
            )),
        ));
    }

    tracing::debug!(
        garage_id = %garage_id,
        namespace = %actual_namespace,
        username = %validated.username,
        "K8s token validated successfully"
    );

    Ok(())
}

/// Validate garage ownership, expiry, and termination status.
///
/// Looks up the garage by ID from the database and verifies:
/// - Garage exists (404 `GARAGE_NOT_FOUND`)
/// - Caller owns the garage (403 `GARAGE_NOT_OWNED`)
/// - Garage is not terminated (410 `GARAGE_TERMINATED`)
/// - Garage TTL has not expired (410 `GARAGE_EXPIRED`)
async fn validate_garage_for_session(
    state: &AppState,
    garage_id: Uuid,
    owner: &str,
) -> Result<DateTime<Utc>, (StatusCode, Json<ApiError>)> {
    let db_garage = garage_repo::get_by_id(&state.db_pool, garage_id)
        .await
        .map_err(|e| {
            if let moto_club_db::DbError::NotFound { .. } = e {
                (
                    StatusCode::NOT_FOUND,
                    Json(ApiError::new(
                        error_codes::GARAGE_NOT_FOUND,
                        format!("Garage '{garage_id}' not found"),
                    )),
                )
            } else {
                tracing::error!(error = %e, garage_id = %garage_id, "Failed to get garage");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(
                        error_codes::INTERNAL_ERROR,
                        format!("Failed to get garage: {e}"),
                    )),
                )
            }
        })?;

    if db_garage.owner != owner {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::GARAGE_NOT_OWNED,
                format!("Garage '{garage_id}' exists but is owned by another user"),
            )),
        ));
    }

    if db_garage.status == GarageStatus::Terminated {
        return Err((
            StatusCode::GONE,
            Json(ApiError::new(
                error_codes::GARAGE_TERMINATED,
                format!("Garage '{garage_id}' has been terminated"),
            )),
        ));
    }

    if db_garage.expires_at < Utc::now() {
        return Err((
            StatusCode::GONE,
            Json(ApiError::new(
                error_codes::GARAGE_EXPIRED,
                format!("Garage '{garage_id}' has expired"),
            )),
        ));
    }

    Ok(db_garage.expires_at)
}

// ============================================================================
// Handlers
// ============================================================================

/// Register a client device.
///
/// POST /api/v1/wg/devices
///
/// The `WireGuard` public key IS the device identity (Cloudflare WARP model).
/// Re-registration with the same key is idempotent.
async fn register_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterDeviceRequest>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // Public key IS the device identity - no separate device_id needed
    let registration = DeviceRegistration {
        public_key: req.public_key,
        owner,
        device_name: req.device_name,
    };

    // Register the device with the peer registry
    let (device, created) = state
        .peer_registry
        .register_device(registration)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to register device");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to register device: {e}"),
                )),
            )
        })?;

    let response = DeviceResponse::from(device);
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((status, Json(response)))
}

/// Get device info.
///
/// GET /`api/v1/wg/devices/{public_key`}
///
/// Note: Public key must be URL-encoded in the path.
async fn get_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(public_key_base64): Path<String>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // Parse the public key from base64
    let public_key = WgPublicKey::from_base64(&public_key_base64).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_PUBLIC_KEY",
                "Invalid public key format",
            )),
        )
    })?;

    // Look up the device in the peer registry
    let device = state.peer_registry.get_device(&public_key).map_err(|e| {
        tracing::error!(error = %e, public_key = %public_key_base64, "Failed to get device");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::INTERNAL_ERROR,
                format!("Failed to get device: {e}"),
            )),
        )
    })?;

    let device = device.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                error_codes::DEVICE_NOT_FOUND,
                format!("Device with public key '{public_key_base64}' not found"),
            )),
        )
    })?;

    // Check device ownership
    if device.owner != owner {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::DEVICE_NOT_OWNED,
                format!("Device '{public_key_base64}' belongs to a different user"),
            )),
        ));
    }

    let response = DeviceResponse::from(device);
    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Create a tunnel session.
///
/// POST /api/v1/wg/sessions
#[allow(clippy::too_many_lines)]
async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // Validate garage ownership, expiry, and termination status
    let garage_expires_at = validate_garage_for_session(&state, req.garage_id, &owner).await?;

    // Look up the device by public key (public key IS the device identity)
    let device = state
        .peer_registry
        .get_device(&req.device_pubkey)
        .map_err(|e| {
            tracing::error!(error = %e, device_pubkey = %req.device_pubkey.to_base64(), "Failed to get device");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to get device: {e}"),
                )),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::DEVICE_NOT_FOUND,
                    format!("Device with public key '{}' not found", req.device_pubkey.to_base64()),
                )),
            )
        })?;

    // Check device ownership
    if device.owner != owner {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ApiError::new(
                error_codes::DEVICE_NOT_OWNED,
                format!(
                    "Device '{}' belongs to a different user",
                    req.device_pubkey.to_base64()
                ),
            )),
        ));
    }

    // Look up the garage WireGuard registration
    let garage_id_str = req.garage_id.to_string();
    let garage = state
        .peer_registry
        .get_garage(&garage_id_str)
        .map_err(|e| {
            tracing::error!(error = %e, garage_id = %req.garage_id, "Failed to get garage");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to get garage: {e}"),
                )),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    error_codes::GARAGE_NOT_REGISTERED,
                    format!(
                        "Garage '{}' hasn't registered its WireGuard endpoint yet",
                        req.garage_id
                    ),
                )),
            )
        })?;

    // Get the DERP map
    let derp_map = state.get_derp_map();

    // Create the session
    let wg_request = WgCreateSessionRequest {
        garage_id: garage_id_str.clone(),
        device_pubkey: req.device_pubkey,
        ttl_seconds: req.ttl_seconds,
        garage_expires_at,
    };

    let response = state
        .session_manager
        .create_session(wg_request, &device, &garage, &derp_map)
        .await
        .map_err(|e| {
            use moto_club_wg::sessions::SessionError;
            if let SessionError::GarageNotRegistered(msg) = &e {
                tracing::warn!(error = %e, "Garage not registered for WireGuard");
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError::new(
                        error_codes::GARAGE_NOT_REGISTERED,
                        msg.clone(),
                    )),
                )
            } else {
                tracing::error!(error = %e, "Failed to create session");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(
                        error_codes::INTERNAL_ERROR,
                        format!("Failed to create session: {e}"),
                    )),
                )
            }
        })?;

    // Notify garages listening on the peer WebSocket
    state.peer_broadcaster.broadcast_add(
        &garage_id_str,
        device.public_key.clone(),
        device.overlay_ip,
    );

    Ok::<_, (StatusCode, Json<ApiError>)>((
        StatusCode::CREATED,
        Json(SessionResponse::from(response)),
    ))
}

/// List active sessions.
///
/// GET /api/v1/wg/sessions
///
/// Lists sessions for the authenticated user.
/// Query parameters:
/// - `garage_id`: Optional filter by garage UUID
/// - `all`: Include expired/closed sessions (default: false)
async fn list_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListSessionsQuery>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // Build filter from query parameters
    let filter = moto_club_db::ListSessionsFilter {
        garage_id: query.garage_id,
        include_all: query.all,
    };

    // Query database for sessions owned by this user
    let db_sessions =
        moto_club_db::wg_session_repo::list_by_owner_with_details(&state.db_pool, &owner, filter)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list sessions");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(
                        error_codes::DATABASE_ERROR,
                        "Failed to list sessions".to_string(),
                    )),
                )
            })?;

    // Convert to response type
    let sessions: Vec<SessionInfo> = db_sessions
        .into_iter()
        .filter_map(|s| {
            let device_pubkey = WgPublicKey::from_base64(&s.device_pubkey).ok()?;
            Some(SessionInfo {
                session_id: format!("sess_{}", s.id.simple()),
                garage_id: s.garage_id.to_string(),
                garage_name: s.garage_name,
                device_pubkey,
                device_name: s.device_name,
                created_at: s.created_at,
                expires_at: s.expires_at,
            })
        })
        .collect();

    let response = ListSessionsResponse { sessions };
    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Close a session.
///
/// DELETE /api/v1/wg/sessions/{id}
async fn close_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let owner = extract_owner(&headers)?;

    // Parse session UUID (strip "sess_" prefix if present)
    let uuid_str = session_id.strip_prefix("sess_").unwrap_or(&session_id);
    let session_uuid = Uuid::parse_str(uuid_str).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                error_codes::SESSION_NOT_FOUND,
                format!("Session '{session_id}' not found"),
            )),
        )
    })?;

    // Verify session ownership before closing
    moto_club_db::wg_session_repo::verify_ownership(&state.db_pool, session_uuid, &owner)
        .await
        .map_err(|e| match e {
            moto_club_db::DbError::NotOwned { .. } => (
                StatusCode::FORBIDDEN,
                Json(ApiError::new(
                    error_codes::SESSION_NOT_OWNED,
                    format!("Session '{session_id}' belongs to a different user"),
                )),
            ),
            _ => (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::SESSION_NOT_FOUND,
                    format!("Session '{session_id}' not found"),
                )),
            ),
        })?;

    // Close the session (idempotent: re-close returns 204)
    let closed_session = state
        .session_manager
        .close_session(&session_id)
        .map_err(|e| {
            tracing::debug!(error = %e, session_id = %session_id, "Failed to close session");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    "Failed to close session".to_string(),
                )),
            )
        })?;

    // Only broadcast removal if the session was actually removed (not a re-close)
    if let Some(session) = closed_session {
        state
            .peer_broadcaster
            .broadcast_remove(&session.garage_id, session.device_pubkey);
    }

    Ok::<_, (StatusCode, Json<ApiError>)>(StatusCode::NO_CONTENT)
}

/// Register a garage (called by garage pod).
///
/// POST /api/v1/wg/garages
///
/// Authorization: Bearer <k8s-service-account-token>
///
/// Validates:
/// 1. K8s `ServiceAccount` token via `TokenReview` API
/// 2. Pod must be in namespace `moto-garage-{garage_id}`
async fn register_garage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterGarageRequest>,
) -> impl IntoResponse {
    // Validate K8s ServiceAccount token and namespace
    validate_garage_token(&state, &headers, &req.garage_id).await?;

    // Verify garage exists in the garages table before upserting into wg_garages.
    // Without this check, the FK constraint on wg_garages.garage_id would produce
    // a generic database error instead of a proper GARAGE_NOT_FOUND 404.
    let garage_uuid = Uuid::parse_str(&req.garage_id).map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                error_codes::GARAGE_NOT_FOUND,
                format!("Garage '{}' not found", req.garage_id),
            )),
        )
    })?;
    garage_repo::get_by_id(&state.db_pool, garage_uuid)
        .await
        .map_err(|e| {
            if let moto_club_db::DbError::NotFound { .. } = e {
                (
                    StatusCode::NOT_FOUND,
                    Json(ApiError::new(
                        error_codes::GARAGE_NOT_FOUND,
                        format!("Garage '{}' not found", req.garage_id),
                    )),
                )
            } else {
                tracing::error!(error = %e, garage_id = %req.garage_id, "Failed to verify garage exists");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError::new(
                        error_codes::INTERNAL_ERROR,
                        format!("Failed to verify garage exists: {e}"),
                    )),
                )
            }
        })?;

    let registration = GarageRegistration {
        garage_id: req.garage_id.clone(),
        public_key: req.public_key,
        endpoints: req.endpoints,
    };

    // Register the garage with the peer registry
    let garage = state
        .peer_registry
        .register_garage(registration)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, garage_id = %req.garage_id, "Failed to register garage");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to register garage: {e}"),
                )),
            )
        })?;

    // Get the DERP map for relay fallback
    let derp_map = state.get_derp_map();

    tracing::info!(
        garage_id = %garage.garage_id,
        overlay_ip = %garage.overlay_ip,
        endpoints = ?garage.endpoints,
        "Garage registered for WireGuard"
    );

    let response = GarageWgResponse {
        assigned_ip: garage.overlay_ip,
        derp_map,
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Get garage `WireGuard` registration.
///
/// GET /`api/v1/wg/garages/{garage_id`}
///
/// Authorization: Bearer <k8s-service-account-token>
///
/// Returns garage's `WireGuard` registration info including public key, assigned IP,
/// endpoints, peer version, and DERP map. Used by garage pods to recover state
/// after restart or check registration status.
///
/// Validates:
/// 1. K8s `ServiceAccount` token via `TokenReview` API
/// 2. Pod must be in namespace `moto-garage-{garage_id}`
async fn get_garage_wg_registration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(garage_id): Path<String>,
) -> impl IntoResponse {
    // Validate K8s ServiceAccount token and namespace
    validate_garage_token(&state, &headers, &garage_id).await?;

    // Look up the garage registration
    let garage = state
        .peer_registry
        .get_garage(&garage_id)
        .map_err(|e| {
            tracing::error!(error = %e, garage_id = %garage_id, "Failed to get garage registration");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to get garage registration: {e}"),
                )),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::GARAGE_NOT_FOUND,
                    format!("Garage '{garage_id}' not found or not registered for WireGuard"),
                )),
            )
        })?;

    // Get the DERP map
    let derp_map = state.get_derp_map();

    tracing::debug!(
        garage_id = %garage.garage_id,
        overlay_ip = %garage.overlay_ip,
        "Retrieved garage WireGuard registration"
    );

    let response = GarageWgRegistrationResponse {
        garage_id: garage.garage_id,
        public_key: garage.public_key,
        assigned_ip: garage.overlay_ip,
        endpoints: garage.endpoints,
        peer_version: garage.peer_version,
        derp_map,
        registered_at: garage.registered_at,
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Get peer list for a garage (garage polls this).
///
/// GET /api/v1/wg/garages/{id}/peers
///
/// Query Parameters:
///   ?version=41 - Optional, return 304 if current version equals this
///
/// Authorization: Bearer <k8s-service-account-token>
///
/// Validates:
/// 1. K8s `ServiceAccount` token via `TokenReview` API
/// 2. Pod must be in namespace `moto-garage-{garage_id}`
///
/// Returns 304 Not Modified if `?version=N` is provided and matches current version.
/// Otherwise returns 200 with peers and current version.
async fn get_garage_peers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(garage_id): Path<String>,
    Query(params): Query<GetPeersParams>,
) -> Response {
    // Validate K8s ServiceAccount token and namespace
    if let Err(err) = validate_garage_token(&state, &headers, &garage_id).await {
        return err.into_response();
    }

    // Get current peer_version from database
    let current_version = match get_peer_version_from_db(&state, &garage_id).await {
        Ok(v) => v,
        Err(err) => return err.into_response(),
    };

    // Conditional GET: return 304 if version matches
    if let Some(client_version) = params.version {
        if client_version == current_version {
            tracing::debug!(
                garage_id = %garage_id,
                version = client_version,
                "Peer list unchanged, returning 304"
            );
            // 304 Not Modified has no body per HTTP spec
            return StatusCode::NOT_MODIFIED.into_response();
        }
    }

    // Get active sessions for this garage and build peer list
    let peers = match build_peer_list(&state, &garage_id) {
        Ok(p) => p,
        Err(err) => return err.into_response(),
    };

    tracing::debug!(
        garage_id = %garage_id,
        peer_count = peers.len(),
        version = current_version,
        "Returning peer list"
    );

    let response = PeerListResponse {
        peers,
        version: current_version,
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Get the current `peer_version` for a garage from the database.
///
/// For in-memory stores (testing), returns 0 as a default.
/// For `PostgreSQL`, queries the `wg_garages` table.
async fn get_peer_version_from_db(
    state: &AppState,
    garage_id: &str,
) -> Result<i32, (StatusCode, Json<ApiError>)> {
    // Try to parse as UUID for database lookup
    let Ok(garage_uuid) = garage_id.parse::<Uuid>() else {
        // For non-UUID garage IDs (in-memory testing), return 0
        return Ok(0);
    };

    // Query the database for peer_version
    let result = moto_club_db::wg_garage_repo::get_peer_version(&state.db_pool, garage_uuid).await;

    match result {
        Ok(version) => Ok(version),
        Err(moto_club_db::DbError::NotFound { .. }) => {
            // Garage not registered yet, return 0
            Ok(0)
        }
        Err(e) => {
            tracing::error!(garage_id = %garage_id, error = %e, "Failed to get peer version");
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to get peer version: {e}"),
                )),
            ))
        }
    }
}

/// Build the peer list from active sessions for a garage.
fn build_peer_list(
    state: &AppState,
    garage_id: &str,
) -> Result<Vec<PeerInfo>, (StatusCode, Json<ApiError>)> {
    // Get active sessions for this garage
    let sessions = state
        .session_manager
        .list_sessions_for_garage(garage_id)
        .map_err(|e| {
            tracing::error!(garage_id = %garage_id, error = %e, "Failed to list sessions");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to list sessions: {e}"),
                )),
            )
        })?;

    // Convert sessions to peer info
    let mut peers = Vec::with_capacity(sessions.len());
    for session in sessions {
        // Look up the device to get its overlay IP
        if let Ok(Some(device)) = state.peer_registry.get_device(&session.device_pubkey) {
            let overlay_ip = device.overlay_ip;
            peers.push(PeerInfo {
                public_key: device.public_key,
                allowed_ip: format!("{overlay_ip}/128"),
            });
        }
    }

    Ok(peers)
}

/// Get DERP server map.
///
/// GET /api/v1/wg/derp-map
///
/// Authorization: Bearer <user-token> OR <k8s-service-account-token>
///
/// Returns the current DERP server map for relay fallback connections.
/// Both clients (via user token) and garages (via K8s service account token)
/// can poll this endpoint to detect DERP server changes.
async fn get_derp_map(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    // Accept either user token or K8s service account token
    // For now, just verify there's some authorization present
    let _has_auth = extract_bearer_token(&headers)?;

    // Get the DERP map
    let derp_map = state.get_derp_map();

    // For v1, version is static since DERP config comes from env var
    // and is loaded at startup. Future versions will track changes.
    let version = 1;

    tracing::debug!(
        region_count = derp_map.len(),
        version = version,
        "Returning DERP map"
    );

    let response = DerpMapResponse { derp_map, version };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

// ============================================================================
// WebSocket Handler
// ============================================================================

/// WebSocket upgrade handler for peer streaming.
///
/// GET /internal/wg/garages/{id}/peers (WebSocket upgrade)
///
/// Authorization: Bearer <k8s-service-account-token>
///
/// Garages maintain a persistent WebSocket connection to receive real-time
/// peer updates when sessions are created or closed.
///
/// Validates:
/// 1. K8s `ServiceAccount` token via `TokenReview` API
/// 2. Pod must be in namespace `moto-garage-{garage_id}`
async fn peers_websocket(
    State(state): State<AppState>,
    Path(garage_id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<axum::response::Response, (StatusCode, Json<ApiError>)> {
    // Validate K8s ServiceAccount token and namespace BEFORE upgrading
    validate_garage_token(&state, &headers, &garage_id).await?;

    tracing::info!(garage_id = %garage_id, "Garage connecting to peer WebSocket");

    Ok(ws.on_upgrade(move |socket| moto_club_ws::handle_peers_socket(socket, garage_id, state)))
}

// ============================================================================
// Router
// ============================================================================

/// Creates the `WireGuard` coordination router.
///
/// Includes:
/// - `POST /api/v1/wg/devices` - Register client device
/// - `GET /api/v1/wg/devices/{public_key}` - Get device info (public key is URL-encoded)
/// - `POST /api/v1/wg/sessions` - Create tunnel session
/// - `GET /api/v1/wg/sessions` - List active sessions
/// - `DELETE /api/v1/wg/sessions/{id}` - Close session
/// - `POST /api/v1/wg/garages` - Register garage
/// - `GET /api/v1/wg/garages/{id}` - Get garage `WireGuard` registration
/// - `GET /api/v1/wg/garages/{id}/peers` - Get peer list
/// - `GET /api/v1/wg/derp-map` - Get DERP server map
/// - `WS /internal/wg/garages/{id}/peers` - Peer streaming WebSocket
pub fn router() -> Router<AppState> {
    Router::new()
        // Device endpoints
        .route("/api/v1/wg/devices", post(register_device))
        .route("/api/v1/wg/devices/{public_key}", get(get_device))
        // Session endpoints
        .route(
            "/api/v1/wg/sessions",
            post(create_session).get(list_sessions),
        )
        .route("/api/v1/wg/sessions/{id}", delete(close_session))
        // Garage WireGuard endpoints
        .route("/api/v1/wg/garages", post(register_garage))
        .route("/api/v1/wg/garages/{id}", get(get_garage_wg_registration))
        .route("/api/v1/wg/garages/{id}/peers", get(get_garage_peers))
        // DERP map endpoint
        .route("/api/v1/wg/derp-map", get(get_derp_map))
        // Internal WebSocket for peer streaming (garages connect here)
        .route("/internal/wg/garages/{id}/peers", get(peers_websocket))
}

#[cfg(test)]
#[path = "wg_test.rs"]
mod tests;
