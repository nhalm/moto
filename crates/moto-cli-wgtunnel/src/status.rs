//! Connection status command for displaying active tunnel sessions.
//!
//! This module provides the [`TunnelStatusInfo`] type for representing tunnel
//! status information, and functions to format status output for the CLI.
//!
//! # Example Output
//!
//! ```text
//! $ moto tunnel status
//!
//! Active Tunnels:
//!
//! SESSION              GARAGE           CLIENT IP           PATH
//! sess_abc123          feature-foo      fd00:moto:2::1      direct (1.2.3.4:51820)
//! sess_def456          bugfix-bar       fd00:moto:2::2      DERP (primary)
//!
//! 2 active tunnel(s)
//! ```
//!
//! # JSON Output
//!
//! When `--json` flag is used:
//!
//! ```json
//! {
//!   "tunnels": [
//!     {
//!       "session_id": "sess_abc123",
//!       "garage_id": "garage_001",
//!       "garage_name": "feature-foo",
//!       "client_ip": "fd00:moto:2::1",
//!       "garage_ip": "fd00:moto:1::abc1",
//!       "status": "connected",
//!       "path_type": "direct",
//!       "path_detail": "1.2.3.4:51820"
//!     }
//!   ],
//!   "count": 1
//! }
//! ```

use serde::Serialize;

use crate::{TunnelManager, TunnelSession, TunnelStatus};
use moto_wgtunnel_conn::PathType;

/// Information about a tunnel session for status display.
///
/// This is a serializable representation of tunnel status suitable
/// for both formatted text output and JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatusInfo {
    /// Session ID from moto-club.
    pub session_id: String,

    /// Garage ID.
    pub garage_id: String,

    /// Garage name (for display).
    pub garage_name: String,

    /// Client's overlay IP address.
    pub client_ip: String,

    /// Garage's overlay IP address.
    pub garage_ip: String,

    /// Current connection status.
    pub status: String,

    /// Path type: "direct", "derp", or "none".
    pub path_type: String,

    /// Path detail: endpoint for direct, region for DERP, or empty.
    pub path_detail: String,
}

impl TunnelStatusInfo {
    /// Create status info from a tunnel session.
    #[must_use]
    pub fn from_session(session: &TunnelSession) -> Self {
        let (status, path_type, path_detail) = match session.status() {
            TunnelStatus::Initializing => (
                "initializing".to_string(),
                "none".to_string(),
                String::new(),
            ),
            TunnelStatus::ConnectingDirect => (
                "connecting".to_string(),
                "direct".to_string(),
                "attempting".to_string(),
            ),
            TunnelStatus::ConnectingDerp { region } => {
                ("connecting".to_string(), "derp".to_string(), region.clone())
            }
            TunnelStatus::Connected { path } => match path {
                PathType::Direct { endpoint } => (
                    "connected".to_string(),
                    "direct".to_string(),
                    endpoint.to_string(),
                ),
                PathType::Derp { region_name, .. } => (
                    "connected".to_string(),
                    "derp".to_string(),
                    region_name.clone(),
                ),
            },
            TunnelStatus::Disconnected => (
                "disconnected".to_string(),
                "none".to_string(),
                String::new(),
            ),
            TunnelStatus::Error { message } => {
                ("error".to_string(), "none".to_string(), message.clone())
            }
        };

        Self {
            session_id: session.session_id().to_string(),
            garage_id: session.garage_id().to_string(),
            garage_name: session.garage_name().to_string(),
            client_ip: session.client_ip().to_string(),
            garage_ip: session.garage_ip().to_string(),
            status,
            path_type,
            path_detail,
        }
    }

    /// Format the path information for display.
    #[must_use]
    pub fn format_path(&self) -> String {
        if self.path_type == "none" {
            "-".to_string()
        } else if self.path_detail.is_empty() {
            self.path_type.clone()
        } else {
            format!("{} ({})", self.path_type, self.path_detail)
        }
    }
}

/// Response structure for tunnel status JSON output.
#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatusResponse {
    /// List of active tunnels.
    pub tunnels: Vec<TunnelStatusInfo>,

    /// Total count of tunnels.
    pub count: usize,
}

impl TunnelStatusResponse {
    /// Create a new status response.
    #[must_use]
    pub fn new(tunnels: Vec<TunnelStatusInfo>) -> Self {
        let count = tunnels.len();
        Self { tunnels, count }
    }
}

/// Get status information for all active tunnels.
///
/// Returns a list of [`TunnelStatusInfo`] for each active tunnel session
/// managed by the tunnel manager.
///
/// # Example
///
/// ```ignore
/// use moto_cli_wgtunnel::{TunnelManager, status::get_tunnel_status};
///
/// let manager = TunnelManager::new().await?;
/// let status = get_tunnel_status(&manager).await;
///
/// for tunnel in &status.tunnels {
///     println!("{}: {}", tunnel.garage_name, tunnel.status);
/// }
/// ```
pub async fn get_tunnel_status(manager: &TunnelManager) -> TunnelStatusResponse {
    let sessions = manager.list_sessions().await;
    let tunnels: Vec<TunnelStatusInfo> = sessions
        .iter()
        .map(TunnelStatusInfo::from_session)
        .collect();

    TunnelStatusResponse::new(tunnels)
}

/// Format tunnel status for human-readable output.
///
/// Returns a formatted string suitable for terminal display.
pub fn format_status_table(response: &TunnelStatusResponse) -> String {
    if response.tunnels.is_empty() {
        return "No active tunnels.".to_string();
    }

    let mut output = String::new();
    output.push_str("Active Tunnels:\n\n");

    // Calculate column widths
    let session_width = response
        .tunnels
        .iter()
        .map(|t| t.session_id.len())
        .max()
        .unwrap_or(7)
        .max(7);

    let garage_width = response
        .tunnels
        .iter()
        .map(|t| t.garage_name.len())
        .max()
        .unwrap_or(6)
        .max(6);

    let client_ip_width = response
        .tunnels
        .iter()
        .map(|t| t.client_ip.len())
        .max()
        .unwrap_or(9)
        .max(9);

    let status_width = response
        .tunnels
        .iter()
        .map(|t| t.status.len())
        .max()
        .unwrap_or(6)
        .max(6);

    // Header
    output.push_str(&format!(
        "{:<session_width$}  {:<garage_width$}  {:<client_ip_width$}  {:<status_width$}  PATH\n",
        "SESSION", "GARAGE", "CLIENT IP", "STATUS"
    ));

    // Rows
    for tunnel in &response.tunnels {
        output.push_str(&format!(
            "{:<session_width$}  {:<garage_width$}  {:<client_ip_width$}  {:<status_width$}  {}\n",
            tunnel.session_id,
            tunnel.garage_name,
            tunnel.client_ip,
            tunnel.status,
            tunnel.format_path()
        ));
    }

    output.push('\n');
    output.push_str(&format!("{} active tunnel(s)\n", response.count));

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TunnelSession;
    use moto_wgtunnel_types::{DerpMap, OverlayIp, WgPrivateKey};

    fn test_session(
        session_id: &str,
        garage_id: &str,
        garage_name: &str,
        status: TunnelStatus,
    ) -> TunnelSession {
        let garage_key = WgPrivateKey::generate().public_key();
        let mut session = TunnelSession::new(
            session_id.to_string(),
            garage_id.to_string(),
            garage_name.to_string(),
            OverlayIp::client(1),
            OverlayIp::garage(1),
            garage_key,
            DerpMap::new(),
        );
        session.set_status(status);
        session
    }

    #[test]
    fn status_info_from_initializing_session() {
        let session = test_session(
            "sess_1",
            "garage_1",
            "test-garage",
            TunnelStatus::Initializing,
        );
        let info = TunnelStatusInfo::from_session(&session);

        assert_eq!(info.session_id, "sess_1");
        assert_eq!(info.garage_name, "test-garage");
        assert_eq!(info.status, "initializing");
        assert_eq!(info.path_type, "none");
        assert!(info.path_detail.is_empty());
        assert_eq!(info.format_path(), "-");
    }

    #[test]
    fn status_info_from_connecting_direct_session() {
        let session = test_session(
            "sess_1",
            "garage_1",
            "test-garage",
            TunnelStatus::ConnectingDirect,
        );
        let info = TunnelStatusInfo::from_session(&session);

        assert_eq!(info.status, "connecting");
        assert_eq!(info.path_type, "direct");
        assert_eq!(info.path_detail, "attempting");
    }

    #[test]
    fn status_info_from_connecting_derp_session() {
        let session = test_session(
            "sess_1",
            "garage_1",
            "test-garage",
            TunnelStatus::ConnectingDerp {
                region: "us-west".to_string(),
            },
        );
        let info = TunnelStatusInfo::from_session(&session);

        assert_eq!(info.status, "connecting");
        assert_eq!(info.path_type, "derp");
        assert_eq!(info.path_detail, "us-west");
    }

    #[test]
    fn status_info_from_connected_direct_session() {
        let session = test_session(
            "sess_1",
            "garage_1",
            "test-garage",
            TunnelStatus::Connected {
                path: PathType::Direct {
                    endpoint: "1.2.3.4:51820".parse().unwrap(),
                },
            },
        );
        let info = TunnelStatusInfo::from_session(&session);

        assert_eq!(info.status, "connected");
        assert_eq!(info.path_type, "direct");
        assert_eq!(info.path_detail, "1.2.3.4:51820");
        assert_eq!(info.format_path(), "direct (1.2.3.4:51820)");
    }

    #[test]
    fn status_info_from_connected_derp_session() {
        let session = test_session(
            "sess_1",
            "garage_1",
            "test-garage",
            TunnelStatus::Connected {
                path: PathType::derp(1, "primary"),
            },
        );
        let info = TunnelStatusInfo::from_session(&session);

        assert_eq!(info.status, "connected");
        assert_eq!(info.path_type, "derp");
        assert_eq!(info.path_detail, "primary");
        assert_eq!(info.format_path(), "derp (primary)");
    }

    #[test]
    fn status_info_from_disconnected_session() {
        let session = test_session(
            "sess_1",
            "garage_1",
            "test-garage",
            TunnelStatus::Disconnected,
        );
        let info = TunnelStatusInfo::from_session(&session);

        assert_eq!(info.status, "disconnected");
        assert_eq!(info.path_type, "none");
        assert_eq!(info.format_path(), "-");
    }

    #[test]
    fn status_info_from_error_session() {
        let session = test_session(
            "sess_1",
            "garage_1",
            "test-garage",
            TunnelStatus::Error {
                message: "connection timeout".to_string(),
            },
        );
        let info = TunnelStatusInfo::from_session(&session);

        assert_eq!(info.status, "error");
        assert_eq!(info.path_type, "none");
        assert_eq!(info.path_detail, "connection timeout");
    }

    #[test]
    fn tunnel_status_response_creation() {
        let tunnels = vec![
            TunnelStatusInfo {
                session_id: "sess_1".to_string(),
                garage_id: "garage_1".to_string(),
                garage_name: "test-1".to_string(),
                client_ip: "fd00:moto:2::1".to_string(),
                garage_ip: "fd00:moto:1::1".to_string(),
                status: "connected".to_string(),
                path_type: "direct".to_string(),
                path_detail: "1.2.3.4:51820".to_string(),
            },
            TunnelStatusInfo {
                session_id: "sess_2".to_string(),
                garage_id: "garage_2".to_string(),
                garage_name: "test-2".to_string(),
                client_ip: "fd00:moto:2::2".to_string(),
                garage_ip: "fd00:moto:1::2".to_string(),
                status: "connected".to_string(),
                path_type: "derp".to_string(),
                path_detail: "primary".to_string(),
            },
        ];

        let response = TunnelStatusResponse::new(tunnels);
        assert_eq!(response.count, 2);
        assert_eq!(response.tunnels.len(), 2);
    }

    #[test]
    fn format_empty_status_table() {
        let response = TunnelStatusResponse::new(vec![]);
        let output = format_status_table(&response);
        assert_eq!(output, "No active tunnels.");
    }

    #[test]
    fn format_status_table_with_tunnels() {
        let tunnels = vec![TunnelStatusInfo {
            session_id: "sess_abc123".to_string(),
            garage_id: "garage_1".to_string(),
            garage_name: "feature-foo".to_string(),
            client_ip: "fd00:moto:2::1".to_string(),
            garage_ip: "fd00:moto:1::1".to_string(),
            status: "connected".to_string(),
            path_type: "direct".to_string(),
            path_detail: "1.2.3.4:51820".to_string(),
        }];

        let response = TunnelStatusResponse::new(tunnels);
        let output = format_status_table(&response);

        assert!(output.contains("Active Tunnels:"));
        assert!(output.contains("SESSION"));
        assert!(output.contains("GARAGE"));
        assert!(output.contains("CLIENT IP"));
        assert!(output.contains("STATUS"));
        assert!(output.contains("PATH"));
        assert!(output.contains("sess_abc123"));
        assert!(output.contains("feature-foo"));
        assert!(output.contains("fd00:moto:2::1"));
        assert!(output.contains("connected"));
        assert!(output.contains("direct (1.2.3.4:51820)"));
        assert!(output.contains("1 active tunnel(s)"));
    }
}
