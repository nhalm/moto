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
//! - `POST /api/v1/users/ssh-keys` - Register user SSH key

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use moto_club_wg::{
    CreateSessionRequest as WgCreateSessionRequest, CreateSessionResponse, DeviceRegistration,
    GarageConnectionInfo, GarageRegistration, RegisteredDevice, RegisteredGarage, Session,
    SshKeyRegistration, SshKeyResponse,
};
use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPublicKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error_codes, ApiError, AppState};

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
#[derive(Debug, Clone, Serialize)]
pub struct DeviceResponse {
    /// Unique device identifier.
    pub device_id: Uuid,
    /// Device's `WireGuard` public key.
    pub public_key: WgPublicKey,
    /// Assigned overlay IP address.
    pub overlay_ip: OverlayIp,
    /// Optional human-readable device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

impl From<RegisteredDevice> for DeviceResponse {
    fn from(d: RegisteredDevice) -> Self {
        Self {
            device_id: d.device_id,
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
    pub garage_id: String,
    /// Device requesting the connection.
    pub device_id: Uuid,
    /// Optional session TTL in seconds. Defaults to garage TTL or 4 hours.
    pub ttl_seconds: Option<u32>,
}

/// Response for session creation.
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
pub struct GarageWgResponse {
    /// Garage identifier.
    pub garage_id: String,
    /// Garage's `WireGuard` public key.
    pub public_key: WgPublicKey,
    /// Assigned overlay IP address.
    pub overlay_ip: OverlayIp,
    /// Direct UDP endpoints for P2P connections.
    pub endpoints: Vec<SocketAddr>,
}

impl From<RegisteredGarage> for GarageWgResponse {
    fn from(g: RegisteredGarage) -> Self {
        Self {
            garage_id: g.garage_id,
            public_key: g.public_key,
            overlay_ip: g.overlay_ip,
            endpoints: g.endpoints,
        }
    }
}

/// Response for peer list (garage polling).
#[derive(Debug, Clone, Serialize)]
pub struct PeerListResponse {
    /// Peers authorized to connect to this garage.
    pub peers: Vec<PeerInfo>,
}

/// Information about an authorized peer.
#[derive(Debug, Clone, Serialize)]
pub struct PeerInfo {
    /// Peer's `WireGuard` public key.
    pub public_key: WgPublicKey,
    /// Peer's allowed IP (their overlay IP).
    pub allowed_ip: String,
}

/// Request to register an SSH key.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterSshKeyRequest {
    /// SSH public key in OpenSSH format.
    pub public_key: String,
}

/// Response for SSH key registration.
#[derive(Debug, Clone, Serialize)]
pub struct SshKeyRegResponse {
    /// Key fingerprint (SHA256 format).
    pub fingerprint: String,
}

impl From<SshKeyResponse> for SshKeyRegResponse {
    fn from(r: SshKeyResponse) -> Self {
        Self {
            fingerprint: r.fingerprint,
        }
    }
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
                Json(ApiError::new("UNAUTHORIZED", "Missing Authorization header")),
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

/// Convert a UUID to a u64 host ID by taking the lower 64 bits.
#[allow(clippy::cast_possible_truncation)]
const fn uuid_to_host_id(id: Uuid) -> u64 {
    id.as_u128() as u64
}

/// Compute a u64 host ID from a string by hashing.
fn string_to_host_id(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

// ============================================================================
// Handlers
// ============================================================================

/// Register a client device.
///
/// POST /api/v1/wg/devices
async fn register_device(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterDeviceRequest>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

    // Generate a device ID for new registrations
    let device_id = Uuid::now_v7();

    let _registration = DeviceRegistration {
        device_id,
        public_key: req.public_key.clone(),
        device_name: req.device_name.clone(),
    };

    // TODO: Use actual PeerRegistry from AppState when moto-club is wired up
    // For now, return a mock response showing the API contract
    let host_id = uuid_to_host_id(device_id);
    let response = DeviceResponse {
        device_id,
        public_key: req.public_key,
        overlay_ip: OverlayIp::client(host_id),
        device_name: req.device_name,
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::CREATED, Json(response)))
}

/// Get device info.
///
/// GET /api/v1/wg/devices/{id}
async fn get_device(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Path(device_id): Path<Uuid>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

    // TODO: Use actual PeerRegistry from AppState when moto-club is wired up
    // For now, return not found to show the API contract
    Err::<(StatusCode, Json<DeviceResponse>), _>((
        StatusCode::NOT_FOUND,
        Json(ApiError::new(
            error_codes::DEVICE_NOT_FOUND,
            format!("Device '{device_id}' not found"),
        )),
    ))
}

/// Create a tunnel session.
///
/// POST /api/v1/wg/sessions
async fn create_session(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

    let _wg_request = WgCreateSessionRequest {
        garage_id: req.garage_id.clone(),
        device_id: req.device_id,
        ttl_seconds: req.ttl_seconds,
    };

    // TODO: Use actual SessionManager and PeerRegistry from AppState
    // For now, return error to indicate garage not registered
    Err::<(StatusCode, Json<SessionResponse>), _>((
        StatusCode::NOT_FOUND,
        Json(ApiError::new(
            error_codes::GARAGE_NOT_FOUND,
            format!(
                "Garage '{}' not found or not registered for WireGuard",
                req.garage_id
            ),
        )),
    ))
}

/// List active sessions.
///
/// GET /api/v1/wg/sessions
async fn list_sessions(
    State(_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

    // TODO: Use actual SessionManager from AppState
    // For now, return empty list
    let response = ListSessionsResponse { sessions: vec![] };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Close a session.
///
/// DELETE /api/v1/wg/sessions/{id}
async fn close_session(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

    // TODO: Use actual SessionManager from AppState
    // For now, return not found
    Err::<StatusCode, _>((
        StatusCode::NOT_FOUND,
        Json(ApiError::new(
            error_codes::SESSION_NOT_FOUND,
            format!("Session '{session_id}' not found"),
        )),
    ))
}

/// Register a garage (called by garage pod).
///
/// POST /api/v1/wg/garages
async fn register_garage(
    State(_state): State<AppState>,
    // Note: Garage registration uses K8s ServiceAccount token, not user Bearer token
    // For now, we'll accept any authentication
    headers: HeaderMap,
    Json(req): Json<RegisterGarageRequest>,
) -> impl IntoResponse {
    // TODO: Validate K8s ServiceAccount token via TokenReview API
    let _ = headers.get("authorization");

    let _registration = GarageRegistration {
        garage_id: req.garage_id.clone(),
        public_key: req.public_key.clone(),
        endpoints: req.endpoints.clone(),
    };

    // TODO: Use actual PeerRegistry from AppState
    // For now, return a mock response showing the API contract
    let host_id = string_to_host_id(&req.garage_id);
    let response = GarageWgResponse {
        garage_id: req.garage_id.clone(),
        public_key: req.public_key,
        overlay_ip: OverlayIp::garage(host_id),
        endpoints: req.endpoints,
    };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::CREATED, Json(response)))
}

/// Get peer list for a garage (garage polls this).
///
/// GET /api/v1/wg/garages/{id}/peers
async fn get_garage_peers(
    State(_state): State<AppState>,
    // Note: Garage uses K8s ServiceAccount token
    headers: HeaderMap,
    Path(garage_id): Path<String>,
) -> impl IntoResponse {
    // TODO: Validate K8s ServiceAccount token
    let _ = headers.get("authorization");

    // TODO: Use actual SessionManager from AppState to get active sessions
    // and return peers for this garage
    // For now, return empty peer list
    let response = PeerListResponse { peers: vec![] };

    // Log for debugging
    tracing::debug!(garage_id = %garage_id, "Garage polling for peers");

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::OK, Json(response)))
}

/// Register an SSH key.
///
/// POST /api/v1/users/ssh-keys
async fn register_ssh_key(
    State(_state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterSshKeyRequest>,
) -> impl IntoResponse {
    let _owner = extract_owner(&headers)?;

    let _registration = SshKeyRegistration {
        public_key: req.public_key.clone(),
    };

    // TODO: Use actual SshKeyManager from AppState
    // For now, compute fingerprint manually (simplified)
    let fingerprint = format!("SHA256:{}", &req.public_key[..20.min(req.public_key.len())]);

    let response = SshKeyRegResponse { fingerprint };

    Ok::<_, (StatusCode, Json<ApiError>)>((StatusCode::CREATED, Json(response)))
}

// ============================================================================
// Router
// ============================================================================

/// Creates the `WireGuard` coordination router.
///
/// Includes:
/// - `POST /api/v1/wg/devices` - Register client device
/// - `GET /api/v1/wg/devices/{id}` - Get device info
/// - `POST /api/v1/wg/sessions` - Create tunnel session
/// - `GET /api/v1/wg/sessions` - List active sessions
/// - `DELETE /api/v1/wg/sessions/{id}` - Close session
/// - `POST /api/v1/wg/garages` - Register garage
/// - `GET /api/v1/wg/garages/{id}/peers` - Get peer list
/// - `POST /api/v1/users/ssh-keys` - Register SSH key
pub fn router() -> Router<AppState> {
    Router::new()
        // Device endpoints
        .route("/api/v1/wg/devices", post(register_device))
        .route("/api/v1/wg/devices/{id}", get(get_device))
        // Session endpoints
        .route(
            "/api/v1/wg/sessions",
            post(create_session).get(list_sessions),
        )
        .route("/api/v1/wg/sessions/{id}", delete(close_session))
        // Garage WireGuard endpoints
        .route("/api/v1/wg/garages", post(register_garage))
        .route("/api/v1/wg/garages/{id}/peers", get(get_garage_peers))
        // SSH key endpoint
        .route("/api/v1/users/ssh-keys", post(register_ssh_key))
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
        let device_id = Uuid::now_v7();
        let json = format!(
            r#"{{
                "garage_id": "bold-mongoose",
                "device_id": "{}",
                "ttl_seconds": 3600
            }}"#,
            device_id
        );
        let req: CreateSessionRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req.garage_id, "bold-mongoose");
        assert_eq!(req.device_id, device_id);
        assert_eq!(req.ttl_seconds, Some(3600));
    }

    #[test]
    fn create_session_request_optional_ttl() {
        let device_id = Uuid::now_v7();
        let json = format!(
            r#"{{
                "garage_id": "bold-mongoose",
                "device_id": "{}"
            }}"#,
            device_id
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
    fn register_ssh_key_request_deserialize() {
        let json = r#"{"public_key": "ssh-ed25519 AAAA... user@host"}"#;
        let req: RegisterSshKeyRequest = serde_json::from_str(json).unwrap();
        assert!(req.public_key.starts_with("ssh-ed25519"));
    }

    #[test]
    fn device_response_serialize() {
        let key = test_public_key();
        let response = DeviceResponse {
            device_id: Uuid::nil(),
            public_key: key,
            overlay_ip: OverlayIp::client(1),
            device_name: Some("test".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("device_id"));
        assert!(json.contains("overlay_ip"));
    }

    #[test]
    fn list_sessions_response_serialize() {
        let response = ListSessionsResponse { sessions: vec![] };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("sessions"));
        assert!(json.contains("[]"));
    }

    #[test]
    fn peer_list_response_serialize() {
        let response = PeerListResponse { peers: vec![] };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("peers"));
    }

    #[test]
    fn ssh_key_reg_response_serialize() {
        let response = SshKeyRegResponse {
            fingerprint: "SHA256:abc123".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("SHA256:abc123"));
    }
}
