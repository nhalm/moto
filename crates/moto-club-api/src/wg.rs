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
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use moto_club_wg::{
    CreateSessionRequest as WgCreateSessionRequest, CreateSessionResponse, DeviceRegistration,
    GarageConnectionInfo, GarageRegistration, PeerEvent, RegisteredDevice, Session,
};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ApiError, AppState, error_codes};
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
/// The WireGuard public key IS the device identity (Cloudflare WARP model).
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
}

impl From<RegisteredDevice> for DeviceResponse {
    fn from(d: RegisteredDevice) -> Self {
        Self {
            public_key: d.public_key,
            overlay_ip: d.overlay_ip,
            device_name: d.device_name,
        }
    }
}

/// Request to create a tunnel session.
#[derive(Debug, Clone, Deserialize)]
pub struct CreateSessionRequest {
    /// Garage to connect to (name or ID).
    pub garage_id: Uuid,
    /// Device requesting the connection (WireGuard public key IS the device identity).
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
    /// Device public key (WireGuard public key IS the device identity).
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

/// Response for getting garage WireGuard registration.
///
/// GET /api/v1/wg/garages/{garage_id}
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
/// Note: Only Serialize is derived because `#[serde(flatten)]` with DerpMap's
/// HashMap<u16, _> regions field causes deserialization issues due to JSON
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

/// Validates K8s ServiceAccount token and verifies the pod is in the expected namespace.
///
/// Per spec (lines 585-603):
/// 1. K8s ServiceAccount token validated via TokenReview API
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
    let k8s_client = match &state.k8s_client {
        Some(client) => client,
        None => {
            tracing::debug!(garage_id = %garage_id, "K8s token validation skipped (no client configured)");
            return Ok(());
        }
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

    // Expected namespace format: moto-garage-{garage_id}
    let expected_namespace = format!("moto-garage-{garage_id}");

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
                    "Pod not running in expected namespace: expected '{}', got '{}'",
                    expected_namespace, actual_namespace
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

// ============================================================================
// Handlers
// ============================================================================

/// Register a client device.
///
/// POST /api/v1/wg/devices
///
/// The WireGuard public key IS the device identity (Cloudflare WARP model).
/// Re-registration with the same key is idempotent.
async fn register_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterDeviceRequest>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

    // Public key IS the device identity - no separate device_id needed
    let registration = DeviceRegistration {
        public_key: req.public_key,
        device_name: req.device_name,
    };

    // Register the device with the peer registry
    let device = state
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

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::CREATED, Json(response)))
}

/// Get device info.
///
/// GET /api/v1/wg/devices/{public_key}
///
/// Note: Public key must be URL-encoded in the path.
async fn get_device(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(public_key_base64): Path<String>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

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

    device.map_or_else(
        || {
            Err((
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::DEVICE_NOT_FOUND,
                    format!("Device with public key '{}' not found", public_key_base64),
                )),
            ))
        },
        |d| {
            let response = DeviceResponse::from(d);
            Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
        },
    )
}

/// Create a tunnel session.
///
/// POST /api/v1/wg/sessions
async fn create_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

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

    // Look up the garage by ID
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
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::GARAGE_NOT_FOUND,
                    format!(
                        "Garage '{}' not found or not registered for WireGuard",
                        req.garage_id
                    ),
                )),
            )
        })?;

    // Get the DERP map
    let derp_map = state.derp_manager.get_map().map_err(|e| {
        tracing::error!(error = %e, "Failed to get DERP map");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::INTERNAL_ERROR,
                format!("Failed to get DERP map: {e}"),
            )),
        )
    })?;

    // Create the session
    let wg_request = WgCreateSessionRequest {
        garage_id: garage_id_str,
        device_pubkey: req.device_pubkey,
        ttl_seconds: req.ttl_seconds,
    };

    let response = state
        .session_manager
        .create_session(wg_request, &device, &garage, &derp_map)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to create session");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError::new(
                    error_codes::INTERNAL_ERROR,
                    format!("Failed to create session: {e}"),
                )),
            )
        })?;

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
    let _owner = extract_owner(&headers)?;

    // Close the session
    state
        .session_manager
        .close_session(&session_id)
        .map_err(|e| {
            tracing::debug!(error = %e, session_id = %session_id, "Failed to close session");
            (
                StatusCode::NOT_FOUND,
                Json(ApiError::new(
                    error_codes::SESSION_NOT_FOUND,
                    format!("Session '{session_id}' not found"),
                )),
            )
        })?;

    Ok::<_, (StatusCode, Json<ApiError>)>(StatusCode::NO_CONTENT)
}

/// Register a garage (called by garage pod).
///
/// POST /api/v1/wg/garages
///
/// Authorization: Bearer <k8s-service-account-token>
///
/// Validates:
/// 1. K8s ServiceAccount token via TokenReview API
/// 2. Pod must be in namespace `moto-garage-{garage_id}`
async fn register_garage(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterGarageRequest>,
) -> impl IntoResponse {
    // Validate K8s ServiceAccount token and namespace
    validate_garage_token(&state, &headers, &req.garage_id).await?;

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
    let derp_map = state.derp_manager.get_map().map_err(|e| {
        tracing::error!(error = %e, garage_id = %req.garage_id, "Failed to get DERP map");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::INTERNAL_ERROR,
                format!("Failed to get DERP map: {e}"),
            )),
        )
    })?;

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

/// Get garage WireGuard registration.
///
/// GET /api/v1/wg/garages/{garage_id}
///
/// Authorization: Bearer <k8s-service-account-token>
///
/// Returns garage's WireGuard registration info including public key, assigned IP,
/// endpoints, peer version, and DERP map. Used by garage pods to recover state
/// after restart or check registration status.
///
/// Validates:
/// 1. K8s ServiceAccount token via TokenReview API
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
                    format!("Garage '{}' not found or not registered for WireGuard", garage_id),
                )),
            )
        })?;

    // Get the DERP map
    let derp_map = state.derp_manager.get_map().map_err(|e| {
        tracing::error!(error = %e, garage_id = %garage_id, "Failed to get DERP map");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::INTERNAL_ERROR,
                format!("Failed to get DERP map: {e}"),
            )),
        )
    })?;

    tracing::debug!(
        garage_id = %garage.garage_id,
        overlay_ip = %garage.overlay_ip,
        "Retrieved garage WireGuard registration"
    );

    // For in-memory store, we use defaults for peer_version and registered_at.
    // The PostgreSQL store will provide real values.
    let response = GarageWgRegistrationResponse {
        garage_id: garage.garage_id,
        public_key: garage.public_key,
        assigned_ip: garage.overlay_ip,
        endpoints: garage.endpoints,
        peer_version: 0, // Default for in-memory; PostgreSQL will provide actual value
        derp_map,
        registered_at: Utc::now(), // Default for in-memory; PostgreSQL will provide actual value
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
/// 1. K8s ServiceAccount token via TokenReview API
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
    let garage_uuid = match garage_id.parse::<Uuid>() {
        Ok(uuid) => uuid,
        Err(_) => {
            // For non-UUID garage IDs (in-memory testing), return 0
            return Ok(0);
        }
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
    let derp_map = state.derp_manager.get_map().map_err(|e| {
        tracing::error!(error = %e, "Failed to get DERP map");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError::new(
                error_codes::INTERNAL_ERROR,
                format!("Failed to get DERP map: {e}"),
            )),
        )
    })?;

    // For v1, version is static since DERP config comes from file
    // and is loaded at startup. Future versions will track changes.
    // TODO: Implement proper versioning when runtime DERP updates are added
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
/// 1. K8s ServiceAccount token via TokenReview API
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

    Ok(ws.on_upgrade(move |socket| handle_peers_socket(socket, garage_id, state)))
}

/// Handle the WebSocket connection for peer streaming.
async fn handle_peers_socket(socket: WebSocket, garage_id: String, state: AppState) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to peer events for this garage
    let mut peer_rx = state.peer_broadcaster.subscribe(&garage_id);

    tracing::info!(garage_id = %garage_id, "Peer WebSocket connected");

    // Send current peers (sessions) to the garage on connect
    if let Ok(sessions) = state.session_manager.list_sessions_for_garage(&garage_id) {
        for session in sessions {
            if let Ok(Some(device)) = state.peer_registry.get_device(&session.device_pubkey) {
                let event = PeerEvent::add(device.public_key, device.overlay_ip);
                if let Ok(json) = event.to_json() {
                    if sender.send(Message::Text(json.into())).await.is_err() {
                        tracing::debug!(garage_id = %garage_id, "Failed to send initial peer");
                        return;
                    }
                }
            }
        }
    }

    loop {
        tokio::select! {
            // Forward peer events to the WebSocket
            result = peer_rx.recv() => {
                match result {
                    Ok(event) => {
                        match event.to_json() {
                            Ok(json) => {
                                if sender.send(Message::Text(json.into())).await.is_err() {
                                    tracing::debug!(garage_id = %garage_id, "WebSocket send failed, closing");
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::error!(garage_id = %garage_id, error = %e, "Failed to serialize peer event");
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(garage_id = %garage_id, lagged = n, "Peer events lagged, some events dropped");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::debug!(garage_id = %garage_id, "Peer broadcast channel closed");
                        break;
                    }
                }
            }
            // Handle incoming WebSocket messages (pings, close, etc.)
            result = receiver.next() => {
                match result {
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!(garage_id = %garage_id, "Peer WebSocket closed by client");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!(garage_id = %garage_id, error = %e, "WebSocket error");
                        break;
                    }
                    _ => {
                        // Ignore text/binary messages from garage
                    }
                }
            }
        }
    }

    // Cleanup when WebSocket closes
    state.peer_broadcaster.remove_garage(&garage_id);
    tracing::info!(garage_id = %garage_id, "Peer WebSocket disconnected");
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
/// - `GET /api/v1/wg/garages/{id}` - Get garage WireGuard registration
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
mod tests {
    use super::*;
    use moto_wgtunnel_types::WgPrivateKey;

    // Helper to generate a valid public key
    fn test_public_key() -> WgPublicKey {
        WgPrivateKey::generate().public_key()
    }

    #[test]
    fn register_device_request_deserialize() {
        let key = test_public_key();
        let json = format!(
            r#"{{
            "public_key": "{}",
            "device_name": "my-laptop"
        }}"#,
            key.to_base64()
        );
        let req: RegisterDeviceRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.device_name, Some("my-laptop".to_string()));
    }

    #[test]
    fn register_device_request_optional_name() {
        let key = test_public_key();
        let json = format!(r#"{{"public_key": "{}"}}"#, key.to_base64());
        let req: RegisterDeviceRequest = serde_json::from_str(&json).unwrap();
        assert!(req.device_name.is_none());
    }

    #[test]
    fn create_session_request_deserialize() {
        let garage_id = Uuid::now_v7();
        let device_key = test_public_key();
        let json = format!(
            r#"{{
                "garage_id": "{}",
                "device_pubkey": "{}",
                "ttl_seconds": 3600
            }}"#,
            garage_id,
            device_key.to_base64()
        );
        let req: CreateSessionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.garage_id, garage_id);
        assert_eq!(req.device_pubkey, device_key);
        assert_eq!(req.ttl_seconds, Some(3600));
    }

    #[test]
    fn create_session_request_optional_ttl() {
        let garage_id = Uuid::now_v7();
        let device_key = test_public_key();
        let json = format!(
            r#"{{
                "garage_id": "{}",
                "device_pubkey": "{}"
            }}"#,
            garage_id,
            device_key.to_base64()
        );
        let req: CreateSessionRequest = serde_json::from_str(&json).unwrap();
        assert!(req.ttl_seconds.is_none());
    }

    #[test]
    fn register_garage_request_deserialize() {
        let key = test_public_key();
        let json = format!(
            r#"{{
            "garage_id": "feature-foo",
            "public_key": "{}",
            "endpoints": ["10.0.0.1:51820"]
        }}"#,
            key.to_base64()
        );
        let req: RegisterGarageRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.garage_id, "feature-foo");
        assert_eq!(req.endpoints.len(), 1);
    }

    #[test]
    fn register_garage_request_no_endpoints() {
        let key = test_public_key();
        let json = format!(
            r#"{{
            "garage_id": "feature-foo",
            "public_key": "{}"
        }}"#,
            key.to_base64()
        );
        let req: RegisterGarageRequest = serde_json::from_str(&json).unwrap();
        assert!(req.endpoints.is_empty());
    }

    #[test]
    fn device_response_serialize() {
        let key = test_public_key();
        let response = DeviceResponse {
            public_key: key,
            overlay_ip: OverlayIp::client(1),
            device_name: Some("test".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("public_key"));
        assert!(json.contains("assigned_ip")); // Note: renamed from overlay_ip
    }

    #[test]
    fn list_sessions_response_serialize() {
        let response = ListSessionsResponse { sessions: vec![] };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("sessions"));
        assert!(json.contains("[]"));
    }

    #[test]
    fn list_sessions_query_defaults() {
        let query: ListSessionsQuery = serde_json::from_str("{}").unwrap();
        assert!(query.garage_id.is_none());
        assert!(!query.all);
    }

    #[test]
    fn list_sessions_query_with_garage_id() {
        let json = r#"{"garage_id": "01234567-89ab-cdef-0123-456789abcdef"}"#;
        let query: ListSessionsQuery = serde_json::from_str(json).unwrap();
        assert!(query.garage_id.is_some());
        assert_eq!(
            query.garage_id.unwrap().to_string(),
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        assert!(!query.all);
    }

    #[test]
    fn list_sessions_query_with_all() {
        let json = r#"{"all": true}"#;
        let query: ListSessionsQuery = serde_json::from_str(json).unwrap();
        assert!(query.garage_id.is_none());
        assert!(query.all);
    }

    #[test]
    fn list_sessions_query_with_both() {
        let json = r#"{"garage_id": "01234567-89ab-cdef-0123-456789abcdef", "all": true}"#;
        let query: ListSessionsQuery = serde_json::from_str(json).unwrap();
        assert!(query.garage_id.is_some());
        assert!(query.all);
    }

    #[test]
    fn peer_list_response_serialize() {
        let response = PeerListResponse {
            peers: vec![],
            version: 42,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("peers"));
        assert!(json.contains(r#""version":42"#));
    }

    #[test]
    fn derp_map_response_serialize() {
        use moto_wgtunnel_types::derp::{DerpNode, DerpRegion};

        let derp_map = DerpMap::new().with_region(
            DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("derp.example.com")),
        );
        let response = DerpMapResponse {
            derp_map,
            version: 5,
        };
        let json = serde_json::to_string(&response).unwrap();

        // Should have regions (flattened from DerpMap)
        assert!(json.contains("regions"));
        // Should have version
        assert!(json.contains(r#""version":5"#));
        // Should have the region data
        assert!(json.contains("primary"));
        assert!(json.contains("derp.example.com"));
    }

    #[test]
    fn derp_map_response_matches_spec_format() {
        // Verify response format matches the spec:
        // {
        //   "regions": { "1": { "name": "primary", "nodes": [...] } },
        //   "version": 5
        // }
        use moto_wgtunnel_types::derp::{DerpNode, DerpRegion};

        let derp_map = DerpMap::new().with_region(
            DerpRegion::new(1, "primary").with_node(DerpNode::new("derp.example.com", 443, 3478)),
        );
        let response = DerpMapResponse {
            derp_map,
            version: 1,
        };

        let json = serde_json::to_string_pretty(&response).unwrap();
        // Parse back to verify structure
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed["regions"].is_object());
        assert!(parsed["regions"]["1"].is_object());
        assert_eq!(parsed["regions"]["1"]["name"], "primary");
        assert!(parsed["regions"]["1"]["nodes"].is_array());
        assert_eq!(parsed["version"], 1);
    }

    mod handler_tests {
        use super::*;
        use crate::AppState;
        use axum::{
            body::Body,
            http::{Request, header},
        };
        use moto_club_wg::{
            DerpMapManager, InMemoryDerpStore, InMemoryPeerStore, InMemorySessionStore,
            InMemoryStore, Ipam, PeerBroadcaster, PeerRegistry, SessionManager,
        };
        use sqlx::postgres::PgPoolOptions;
        use std::sync::Arc;
        use tower::ServiceExt;

        fn create_test_state() -> AppState {
            let ipam_store = InMemoryStore::new();
            let peer_store = InMemoryPeerStore::new();
            let session_store = InMemorySessionStore::new();
            let derp_store = InMemoryDerpStore::with_default_map();

            let ipam = Ipam::new(ipam_store);
            let peer_registry = Arc::new(PeerRegistry::new(peer_store, ipam));
            let session_manager = Arc::new(SessionManager::new(session_store));
            let derp_manager = Arc::new(DerpMapManager::new(derp_store));
            let peer_broadcaster = Arc::new(PeerBroadcaster::new());

            // Create a pool that will never actually connect (WG endpoints don't use DB)
            let db_pool = PgPoolOptions::new()
                .max_connections(1)
                .connect_lazy("postgres://unused:unused@localhost/unused")
                .unwrap();
            AppState::new(
                db_pool,
                peer_registry,
                session_manager,
                derp_manager,
                peer_broadcaster,
            )
        }

        #[tokio::test]
        async fn register_device_success() {
            let state = create_test_state();
            let app = router().with_state(state);

            let key = test_public_key();
            let body = serde_json::json!({
                "public_key": key.to_base64(),
                "device_name": "test-laptop"
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/devices")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::CREATED);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let device: DeviceResponse = serde_json::from_slice(&body).unwrap();

            assert_eq!(device.device_name, Some("test-laptop".to_string()));
            assert!(device.overlay_ip.is_client());
        }

        #[tokio::test]
        async fn register_device_without_name() {
            let state = create_test_state();
            let app = router().with_state(state);

            let key = test_public_key();
            let body = serde_json::json!({
                "public_key": key.to_base64()
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/devices")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::CREATED);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let device: DeviceResponse = serde_json::from_slice(&body).unwrap();

            assert!(device.device_name.is_none());
        }

        #[tokio::test]
        async fn register_device_requires_auth() {
            let state = create_test_state();
            let app = router().with_state(state);

            let key = test_public_key();
            let body = serde_json::json!({
                "public_key": key.to_base64()
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/devices")
                        .header(header::CONTENT_TYPE, "application/json")
                        // No authorization header
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn get_device_not_found() {
            let state = create_test_state();
            let app = router().with_state(state);

            // Use a non-existent public key (base64 is URL-safe except for + and /)
            // We'll percent-encode the key manually for the test
            let nonexistent_key = test_public_key();
            let key_base64 = nonexistent_key.to_base64();
            // URL-encode the base64 string (replace + with %2B, / with %2F, = with %3D)
            let key_encoded = key_base64
                .replace('+', "%2B")
                .replace('/', "%2F")
                .replace('=', "%3D");
            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!("/api/v1/wg/devices/{}", key_encoded))
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn register_then_get_device() {
            let state = create_test_state();
            let peer_registry = state.peer_registry.clone();
            let app = router().with_state(state);

            // Register a device
            let key = test_public_key();
            let body = serde_json::json!({
                "public_key": key.to_base64(),
                "device_name": "test-device"
            });

            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/devices")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::CREATED);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let registered: DeviceResponse = serde_json::from_slice(&body).unwrap();

            // Now get the device - use the peer_registry directly since we need state
            let device = peer_registry.get_device(&registered.public_key).unwrap();
            assert!(device.is_some());
            let device = device.unwrap();
            assert_eq!(device.public_key, registered.public_key);
            assert_eq!(device.overlay_ip, registered.overlay_ip);
        }

        #[tokio::test]
        async fn create_session_device_not_found() {
            let state = create_test_state();
            let app = router().with_state(state);

            // Use an unregistered device public key
            let device_key = test_public_key();
            let garage_id = Uuid::now_v7();
            let body = serde_json::json!({
                "garage_id": garage_id.to_string(),
                "device_pubkey": device_key.to_base64()
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/sessions")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn create_session_garage_not_found() {
            let state = create_test_state();
            let peer_registry = state.peer_registry.clone();
            let app = router().with_state(state);

            // Register a device first
            let device_key = test_public_key();
            peer_registry
                .register_device(moto_club_wg::DeviceRegistration {
                    public_key: device_key.clone(),
                    device_name: Some("test-device".to_string()),
                })
                .await
                .unwrap();

            let nonexistent_garage_id = Uuid::now_v7();
            let body = serde_json::json!({
                "garage_id": nonexistent_garage_id.to_string(),
                "device_pubkey": device_key.to_base64()
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/sessions")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn create_session_success() {
            let state = create_test_state();
            let peer_registry = state.peer_registry.clone();
            let app = router().with_state(state);

            // Register a device
            let device_key = test_public_key();
            peer_registry
                .register_device(moto_club_wg::DeviceRegistration {
                    public_key: device_key.clone(),
                    device_name: Some("test-device".to_string()),
                })
                .await
                .unwrap();

            // Register a garage (using UUID as garage_id)
            let garage_id = Uuid::now_v7();
            let garage_key = test_public_key();
            peer_registry
                .register_garage(moto_club_wg::GarageRegistration {
                    garage_id: garage_id.to_string(),
                    public_key: garage_key,
                    endpoints: vec!["10.0.0.1:51820".parse().unwrap()],
                })
                .await
                .unwrap();

            let body = serde_json::json!({
                "garage_id": garage_id.to_string(),
                "device_pubkey": device_key.to_base64(),
                "ttl_seconds": 3600
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/sessions")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::CREATED);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let session: SessionResponse = serde_json::from_slice(&body).unwrap();

            assert!(session.session_id.starts_with("sess_"));
            assert!(session.client_ip.is_client());
            assert!(session.garage.overlay_ip.is_garage());
        }

        #[tokio::test]
        async fn close_session_not_found() {
            let state = create_test_state();
            let app = router().with_state(state);

            let response = app
                .oneshot(
                    Request::builder()
                        .method("DELETE")
                        .uri("/api/v1/wg/sessions/sess_nonexistent")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn register_garage_success() {
            let state = create_test_state();
            let app = router().with_state(state);

            let key = test_public_key();
            let body = serde_json::json!({
                "garage_id": "test-garage",
                "public_key": key.to_base64(),
                "endpoints": ["10.0.0.1:51820"]
            });

            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/garages")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                        .body(Body::from(serde_json::to_string(&body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let garage: GarageWgResponse = serde_json::from_slice(&body).unwrap();

            // Assigned IP should be in the garage subnet
            assert!(garage.assigned_ip.is_garage());
            // DERP map should have at least one region
            assert!(!garage.derp_map.regions().is_empty());
        }

        #[tokio::test]
        async fn get_garage_wg_registration_not_found() {
            let state = create_test_state();
            let app = router().with_state(state);

            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/wg/garages/nonexistent-garage")
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }

        #[tokio::test]
        async fn get_garage_wg_registration_success() {
            let state = create_test_state();
            let peer_registry = state.peer_registry.clone();
            let app = router().with_state(state);

            // Register a garage first
            let garage_id = "test-garage-for-get";
            let key = test_public_key();
            peer_registry
                .register_garage(moto_club_wg::GarageRegistration {
                    garage_id: garage_id.to_string(),
                    public_key: key.clone(),
                    endpoints: vec!["10.0.0.1:51820".parse().unwrap()],
                })
                .await
                .unwrap();

            // Now get the registration
            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!("/api/v1/wg/garages/{}", garage_id))
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let registration: GarageWgRegistrationResponse = serde_json::from_slice(&body).unwrap();

            // Verify response fields
            assert_eq!(registration.garage_id, garage_id);
            assert_eq!(registration.public_key, key);
            assert!(registration.assigned_ip.is_garage());
            assert_eq!(registration.endpoints.len(), 1);
            assert_eq!(registration.peer_version, 0); // Default for in-memory
            assert!(!registration.derp_map.regions().is_empty());
        }

        #[tokio::test]
        async fn create_and_close_session() {
            let state = create_test_state();
            let peer_registry = state.peer_registry.clone();
            let app = router().with_state(state);

            // Register device and garage
            let device_key = test_public_key();
            peer_registry
                .register_device(moto_club_wg::DeviceRegistration {
                    public_key: device_key.clone(),
                    device_name: None,
                })
                .await
                .unwrap();

            let garage_id = Uuid::now_v7();
            let garage_key = test_public_key();
            peer_registry
                .register_garage(moto_club_wg::GarageRegistration {
                    garage_id: garage_id.to_string(),
                    public_key: garage_key,
                    endpoints: vec![],
                })
                .await
                .unwrap();

            // Create session
            let create_body = serde_json::json!({
                "garage_id": garage_id.to_string(),
                "device_pubkey": device_key.to_base64()
            });

            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/api/v1/wg/sessions")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::from(serde_json::to_string(&create_body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::CREATED);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let session: SessionResponse = serde_json::from_slice(&body).unwrap();

            // Close session
            let response = app
                .oneshot(
                    Request::builder()
                        .method("DELETE")
                        .uri(format!("/api/v1/wg/sessions/{}", session.session_id))
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }

        #[tokio::test]
        async fn get_derp_map_success() {
            let state = create_test_state();
            let app = router().with_state(state);

            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/wg/derp-map")
                        .header(header::AUTHORIZATION, "Bearer testuser")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();

            // Parse as generic JSON since DerpMapResponse doesn't implement Deserialize
            // (due to serde(flatten) with HashMap<u16, _> not round-tripping correctly)
            let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

            // Verify version field
            assert_eq!(result["version"], 1);

            // Verify regions field exists and has expected structure
            assert!(result["regions"].is_object());
            let regions = result["regions"].as_object().unwrap();
            assert!(!regions.is_empty());

            // Default test state includes one region with derp.moto.dev
            assert!(regions.contains_key("1"));
            let region1 = &regions["1"];
            assert_eq!(region1["name"], "primary");
            assert!(region1["nodes"].is_array());
            assert_eq!(region1["nodes"][0]["host"], "derp.moto.dev");
        }

        #[tokio::test]
        async fn get_derp_map_requires_auth() {
            let state = create_test_state();
            let app = router().with_state(state);

            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/wg/derp-map")
                        // No authorization header
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn get_derp_map_accepts_k8s_token() {
            // K8s service account tokens should also be accepted
            let state = create_test_state();
            let app = router().with_state(state);

            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/wg/derp-map")
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account-token")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn get_garage_peers_returns_version() {
            let state = create_test_state();
            let app = router().with_state(state);

            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/wg/garages/test-garage/peers")
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

            // Should have peers array and version field
            assert!(result["peers"].is_array());
            assert!(result["version"].is_number());
        }

        #[tokio::test]
        async fn get_garage_peers_conditional_304() {
            let state = create_test_state();
            let app = router().with_state(state);

            // First request without version param
            let response1 = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/wg/garages/test-garage/peers")
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response1.status(), StatusCode::OK);

            let body = axum::body::to_bytes(response1.into_body(), usize::MAX)
                .await
                .unwrap();
            let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let version = result["version"].as_i64().unwrap();

            // Second request with matching version should return 304
            let response2 = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(format!(
                            "/api/v1/wg/garages/test-garage/peers?version={}",
                            version
                        ))
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response2.status(), StatusCode::NOT_MODIFIED);
        }

        #[tokio::test]
        async fn get_garage_peers_conditional_200_on_version_mismatch() {
            let state = create_test_state();
            let app = router().with_state(state);

            // Request with a different version should return 200
            let response = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/v1/wg/garages/test-garage/peers?version=999")
                        .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);

            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

            // Should have peers and version
            assert!(result["peers"].is_array());
            assert!(result["version"].is_number());
        }
    }
}
