//! PostgreSQL-backed storage implementations for WireGuard coordination.
//!
//! This module provides PostgreSQL implementations of the storage traits
//! defined in `moto-club-wg`, using the repositories from `moto-club-db`.

use std::net::SocketAddr;

use moto_club_db::{DbPool, user_ssh_key_repo, wg_device_repo, wg_garage_repo, wg_session_repo};
use moto_club_wg::{
    peers::{PeerError, PeerStore, RegisteredDevice, RegisteredGarage},
    sessions::{Session, SessionError, SessionStore},
    ssh_keys::{RegisteredSshKey, SshKeyError, SshKeyStore},
};
use moto_wgtunnel_types::{OverlayIp, WgPublicKey};
use uuid::Uuid;

// ============================================================================
// PostgreSQL Peer Store
// ============================================================================

/// PostgreSQL-backed peer store for device and garage registration.
///
/// Uses `wg_device_repo` and `wg_garage_repo` from `moto-club-db`.
pub struct PostgresPeerStore {
    pool: DbPool,
}

impl PostgresPeerStore {
    /// Create a new PostgreSQL peer store.
    #[must_use]
    pub const fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl PeerStore for PostgresPeerStore {
    fn get_device(
        &self,
        public_key: &WgPublicKey,
    ) -> moto_club_wg::peers::Result<Option<RegisteredDevice>> {
        let pool = self.pool.clone();
        let public_key_b64 = public_key.to_base64();

        // Use a blocking runtime to call async code from sync trait method
        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { wg_device_repo::get_by_public_key(&pool, &public_key_b64).await })
        });

        match result {
            Ok(device) => {
                // Parse the overlay IP from string
                let overlay_ip = parse_client_overlay_ip(&device.assigned_ip)
                    .map_err(|e| PeerError::Storage(e.to_string()))?;

                // Parse the public key
                let public_key = WgPublicKey::from_base64(&device.public_key)
                    .map_err(|e| PeerError::Storage(format!("invalid public key: {e}")))?;

                Ok(Some(RegisteredDevice {
                    public_key,
                    overlay_ip,
                    device_name: device.device_name,
                }))
            }
            Err(moto_club_db::DbError::NotFound { .. }) => Ok(None),
            Err(e) => Err(PeerError::Storage(e.to_string())),
        }
    }

    fn set_device(&self, device: RegisteredDevice) -> moto_club_wg::peers::Result<()> {
        let pool = self.pool.clone();
        let input = wg_device_repo::CreateWgDevice {
            public_key: device.public_key.to_base64(),
            // Note: We don't have owner info in RegisteredDevice, so we use a placeholder.
            // In practice, the API layer should handle this before calling the trait.
            owner: "unknown".to_string(),
            device_name: device.device_name,
            assigned_ip: device.overlay_ip.to_string(),
        };

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                // Use get_or_create to handle idempotent registration
                wg_device_repo::get_or_create(&pool, input).await
            })
        })
        .map_err(|e| PeerError::Storage(e.to_string()))?;

        Ok(())
    }

    fn get_garage(&self, garage_id: &str) -> moto_club_wg::peers::Result<Option<RegisteredGarage>> {
        let pool = self.pool.clone();
        let garage_uuid = garage_id
            .parse::<Uuid>()
            .map_err(|_| PeerError::GarageNotFound(garage_id.to_string()))?;

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { wg_garage_repo::get_by_garage_id(&pool, garage_uuid).await })
        });

        match result {
            Ok(wg_garage) => {
                // Parse the overlay IP
                let overlay_ip = parse_garage_overlay_ip(&wg_garage.assigned_ip)
                    .map_err(|e| PeerError::Storage(e.to_string()))?;

                // Parse the public key
                let public_key = WgPublicKey::from_base64(&wg_garage.public_key)
                    .map_err(|e| PeerError::Storage(format!("invalid public key: {e}")))?;

                // Parse endpoints
                let endpoints: Vec<SocketAddr> = wg_garage
                    .endpoints
                    .iter()
                    .filter_map(|s| s.parse().ok())
                    .collect();

                Ok(Some(RegisteredGarage {
                    garage_id: garage_id.to_string(),
                    public_key,
                    overlay_ip,
                    endpoints,
                }))
            }
            Err(moto_club_db::DbError::NotFound { .. }) => Ok(None),
            Err(e) => Err(PeerError::Storage(e.to_string())),
        }
    }

    fn set_garage(&self, garage: RegisteredGarage) -> moto_club_wg::peers::Result<()> {
        let pool = self.pool.clone();
        let garage_uuid = garage
            .garage_id
            .parse::<Uuid>()
            .map_err(|_| PeerError::GarageNotFound(garage.garage_id.clone()))?;

        let input = wg_garage_repo::RegisterWgGarage {
            garage_id: garage_uuid,
            public_key: garage.public_key.to_base64(),
            endpoints: garage.endpoints.iter().map(ToString::to_string).collect(),
        };

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { wg_garage_repo::register(&pool, input).await })
        })
        .map_err(|e| PeerError::Storage(e.to_string()))?;

        Ok(())
    }

    fn remove_garage(
        &self,
        garage_id: &str,
    ) -> moto_club_wg::peers::Result<Option<RegisteredGarage>> {
        let pool = self.pool.clone();
        let garage_uuid = garage_id
            .parse::<Uuid>()
            .map_err(|_| PeerError::GarageNotFound(garage_id.to_string()))?;

        // First get the garage, then delete it
        let garage = self.get_garage(garage_id)?;

        if garage.is_some() {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(async { wg_garage_repo::delete(&pool, garage_uuid).await })
            })
            .map_err(|e| PeerError::Storage(e.to_string()))?;
        }

        Ok(garage)
    }

    fn list_garages(&self) -> moto_club_wg::peers::Result<Vec<RegisteredGarage>> {
        // Note: There's no list function in wg_garage_repo, so we return an empty list for now.
        // This could be added if needed.
        Ok(vec![])
    }
}

// ============================================================================
// PostgreSQL Session Store
// ============================================================================

/// PostgreSQL-backed session store for tunnel sessions.
///
/// Uses `wg_session_repo` from `moto-club-db`.
pub struct PostgresSessionStore {
    pool: DbPool,
}

impl PostgresSessionStore {
    /// Create a new PostgreSQL session store.
    #[must_use]
    pub const fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl SessionStore for PostgresSessionStore {
    fn get_session(&self, session_id: &str) -> moto_club_wg::sessions::Result<Option<Session>> {
        let pool = self.pool.clone();

        // Session IDs are prefixed with "sess_" followed by a UUID in simple format
        let uuid_str = session_id.strip_prefix("sess_").unwrap_or(session_id);
        let session_uuid = Uuid::parse_str(uuid_str)
            .map_err(|_| SessionError::NotFound(session_id.to_string()))?;

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { wg_session_repo::get_by_id(&pool, session_uuid).await })
        });

        match result {
            Ok(db_session) => {
                let public_key = WgPublicKey::from_base64(&db_session.device_pubkey)
                    .map_err(|e| SessionError::Storage(format!("invalid public key: {e}")))?;

                Ok(Some(Session {
                    session_id: format!("sess_{}", db_session.id.simple()),
                    garage_id: db_session.garage_id.to_string(),
                    garage_name: db_session.garage_id.to_string(), // Could look up actual name
                    device_pubkey: public_key,
                    created_at: db_session.created_at,
                    expires_at: db_session.expires_at,
                }))
            }
            Err(moto_club_db::DbError::NotFound { .. }) => Ok(None),
            Err(e) => Err(SessionError::Storage(e.to_string())),
        }
    }

    fn set_session(&self, session: Session) -> moto_club_wg::sessions::Result<()> {
        let pool = self.pool.clone();

        // Parse the garage ID as UUID
        let garage_uuid = session.garage_id.parse::<Uuid>().map_err(|_| {
            SessionError::Storage(format!("invalid garage_id: {}", session.garage_id))
        })?;

        let input = wg_session_repo::CreateWgSession {
            device_pubkey: session.device_pubkey.to_base64(),
            garage_id: garage_uuid,
            expires_at: session.expires_at,
        };

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { wg_session_repo::create(&pool, input).await })
        })
        .map_err(|e| SessionError::Storage(e.to_string()))?;

        Ok(())
    }

    fn remove_session(&self, session_id: &str) -> moto_club_wg::sessions::Result<Option<Session>> {
        let pool = self.pool.clone();

        // Get the session first
        let session = self.get_session(session_id)?;

        if let Some(ref s) = session {
            // Parse session ID
            let uuid_str = session_id.strip_prefix("sess_").unwrap_or(session_id);
            if let Ok(session_uuid) = Uuid::parse_str(uuid_str) {
                // Soft-delete by closing the session
                let _ = tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current()
                        .block_on(async { wg_session_repo::close(&pool, session_uuid).await })
                });

                // Increment peer_version for the garage
                if let Ok(garage_uuid) = s.garage_id.parse::<Uuid>() {
                    let _ = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            wg_garage_repo::increment_peer_version(&pool, garage_uuid).await
                        })
                    });
                }
            }
        }

        Ok(session)
    }

    fn list_sessions_by_device(
        &self,
        device_pubkey: &WgPublicKey,
    ) -> moto_club_wg::sessions::Result<Vec<Session>> {
        let pool = self.pool.clone();
        let public_key_b64 = device_pubkey.to_base64();

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                wg_session_repo::list_active_by_device(&pool, &public_key_b64).await
            })
        });

        match result {
            Ok(db_sessions) => {
                let sessions: Vec<Session> = db_sessions
                    .into_iter()
                    .filter_map(|s| {
                        let public_key = WgPublicKey::from_base64(&s.device_pubkey).ok()?;
                        Some(Session {
                            session_id: format!("sess_{}", s.id.simple()),
                            garage_id: s.garage_id.to_string(),
                            garage_name: s.garage_id.to_string(),
                            device_pubkey: public_key,
                            created_at: s.created_at,
                            expires_at: s.expires_at,
                        })
                    })
                    .collect();
                Ok(sessions)
            }
            Err(e) => Err(SessionError::Storage(e.to_string())),
        }
    }

    fn list_sessions_by_garage(
        &self,
        garage_id: &str,
    ) -> moto_club_wg::sessions::Result<Vec<Session>> {
        let pool = self.pool.clone();
        let garage_uuid = garage_id
            .parse::<Uuid>()
            .map_err(|_| SessionError::Storage(format!("invalid garage_id: {garage_id}")))?;

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                wg_session_repo::list_active_by_garage(&pool, garage_uuid).await
            })
        });

        match result {
            Ok(db_sessions) => {
                let sessions: Vec<Session> = db_sessions
                    .into_iter()
                    .filter_map(|s| {
                        let public_key = WgPublicKey::from_base64(&s.device_pubkey).ok()?;
                        Some(Session {
                            session_id: format!("sess_{}", s.id.simple()),
                            garage_id: s.garage_id.to_string(),
                            garage_name: s.garage_id.to_string(),
                            device_pubkey: public_key,
                            created_at: s.created_at,
                            expires_at: s.expires_at,
                        })
                    })
                    .collect();
                Ok(sessions)
            }
            Err(e) => Err(SessionError::Storage(e.to_string())),
        }
    }

    fn remove_sessions_by_garage(
        &self,
        garage_id: &str,
    ) -> moto_club_wg::sessions::Result<Vec<Session>> {
        let pool = self.pool.clone();
        let garage_uuid = garage_id
            .parse::<Uuid>()
            .map_err(|_| SessionError::Storage(format!("invalid garage_id: {garage_id}")))?;

        // First get all sessions for the garage
        let sessions = self.list_sessions_by_garage(garage_id)?;

        // Close all sessions
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { wg_session_repo::close_all_for_garage(&pool, garage_uuid).await })
        })
        .map_err(|e| SessionError::Storage(e.to_string()))?;

        // Increment peer_version
        let _ = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                wg_garage_repo::increment_peer_version(&pool, garage_uuid).await
            })
        });

        Ok(sessions)
    }

    fn remove_expired_sessions(&self) -> moto_club_wg::sessions::Result<Vec<Session>> {
        // Note: This would require a new repo function to get and close expired sessions.
        // For now, return empty - moto-cron handles this cleanup.
        Ok(vec![])
    }
}

// ============================================================================
// PostgreSQL SSH Key Store
// ============================================================================

/// PostgreSQL-backed SSH key store for user key management.
///
/// Uses `user_ssh_key_repo` from `moto-club-db`.
pub struct PostgresSshKeyStore {
    pool: DbPool,
}

impl PostgresSshKeyStore {
    /// Create a new PostgreSQL SSH key store.
    #[must_use]
    pub const fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

impl SshKeyStore for PostgresSshKeyStore {
    fn get_key(&self, key_id: Uuid) -> moto_club_wg::ssh_keys::Result<Option<RegisteredSshKey>> {
        let pool = self.pool.clone();

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { user_ssh_key_repo::get_by_id(&pool, key_id).await })
        });

        match result {
            Ok(db_key) => Ok(Some(db_key_to_registered_key(db_key))),
            Err(moto_club_db::DbError::NotFound { .. }) => Ok(None),
            Err(e) => Err(SshKeyError::Storage(e.to_string())),
        }
    }

    fn get_key_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> moto_club_wg::ssh_keys::Result<Option<RegisteredSshKey>> {
        // Note: This would need to iterate all keys or add a new repo function.
        // For now, we don't have a direct lookup by fingerprint across all users.
        // The SshKeyManager already handles idempotent registration logic.
        let _ = fingerprint;
        Ok(None)
    }

    fn set_key(&self, key: RegisteredSshKey) -> moto_club_wg::ssh_keys::Result<()> {
        let pool = self.pool.clone();

        // Note: The owner in RegisteredSshKey is a UUID (user_id), but the DB uses String.
        // We convert UUID to string for storage.
        let input = user_ssh_key_repo::CreateUserSshKey {
            owner: key.user_id.to_string(),
            public_key: key.public_key,
            fingerprint: key.fingerprint,
        };

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { user_ssh_key_repo::get_or_create(&pool, input).await })
        })
        .map_err(|e| SshKeyError::Storage(e.to_string()))?;

        Ok(())
    }

    fn remove_key(&self, key_id: Uuid) -> moto_club_wg::ssh_keys::Result<Option<RegisteredSshKey>> {
        let pool = self.pool.clone();

        // First get the key
        let key = self.get_key(key_id)?;

        if key.is_some() {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(async { user_ssh_key_repo::delete(&pool, key_id).await })
            })
            .map_err(|e| SshKeyError::Storage(e.to_string()))?;
        }

        Ok(key)
    }

    fn list_keys_by_user(
        &self,
        user_id: Uuid,
    ) -> moto_club_wg::ssh_keys::Result<Vec<RegisteredSshKey>> {
        let pool = self.pool.clone();
        let owner = user_id.to_string();

        let result = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(async { user_ssh_key_repo::list_by_owner(&pool, &owner).await })
        });

        match result {
            Ok(db_keys) => {
                let keys = db_keys.into_iter().map(db_key_to_registered_key).collect();
                Ok(keys)
            }
            Err(e) => Err(SshKeyError::Storage(e.to_string())),
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert a database SSH key to a registered SSH key.
fn db_key_to_registered_key(db_key: moto_club_db::UserSshKey) -> RegisteredSshKey {
    // Parse the owner string back to UUID, defaulting to nil if parsing fails
    let user_id = Uuid::parse_str(&db_key.owner).unwrap_or(Uuid::nil());

    // Extract algorithm and comment from the public key
    let parts: Vec<&str> = db_key.public_key.split_whitespace().collect();
    let algorithm = parts.first().unwrap_or(&"unknown").to_string();
    let comment = if parts.len() > 2 {
        Some(parts[2..].join(" "))
    } else {
        None
    };

    RegisteredSshKey {
        key_id: db_key.id,
        user_id,
        public_key: db_key.public_key,
        fingerprint: db_key.fingerprint,
        algorithm,
        comment,
    }
}

/// Parse a client overlay IP from a string.
fn parse_client_overlay_ip(ip_str: &str) -> Result<OverlayIp, String> {
    // Extract the suffix from fd00:moto:2::xxx
    let suffix = ip_str
        .strip_prefix("fd00:moto:2::")
        .ok_or_else(|| format!("invalid client IP format: {ip_str}"))?;

    let index = u64::from_str_radix(suffix, 16).map_err(|e| format!("invalid IP suffix: {e}"))?;

    Ok(OverlayIp::client(index))
}

/// Parse a garage overlay IP from a string.
///
/// The DB stores IPs in format "fd00:moto:1::{hex}:{hex}:{hex}:{hex}".
/// We need to extract the 64-bit host part and create an OverlayIp.
fn parse_garage_overlay_ip(ip_str: &str) -> Result<OverlayIp, String> {
    // Garage IPs are in fd00:moto:1:: subnet
    let suffix = ip_str
        .strip_prefix("fd00:moto:1::")
        .ok_or_else(|| format!("invalid garage IP format: {ip_str}"))?;

    // Parse the 4 groups of 16-bit hex values
    // Format: a:b:c:d where each is a 16-bit hex value
    let parts: Vec<&str> = suffix.split(':').collect();
    if parts.len() != 4 {
        return Err(format!("invalid garage IP host part: {suffix}"));
    }

    let a =
        u16::from_str_radix(parts[0], 16).map_err(|e| format!("invalid hex in garage IP: {e}"))?;
    let b =
        u16::from_str_radix(parts[1], 16).map_err(|e| format!("invalid hex in garage IP: {e}"))?;
    let c =
        u16::from_str_radix(parts[2], 16).map_err(|e| format!("invalid hex in garage IP: {e}"))?;
    let d =
        u16::from_str_radix(parts[3], 16).map_err(|e| format!("invalid hex in garage IP: {e}"))?;

    // Reconstruct the 64-bit host part
    let host_id = (u64::from(a) << 48) | (u64::from(b) << 32) | (u64::from(c) << 16) | u64::from(d);

    Ok(OverlayIp::garage(host_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_client_ip_valid() {
        let ip = parse_client_overlay_ip("fd00:moto:2::1").unwrap();
        assert!(ip.is_client());
    }

    #[test]
    fn parse_client_ip_hex() {
        let ip = parse_client_overlay_ip("fd00:moto:2::a").unwrap();
        assert!(ip.is_client());
    }

    #[test]
    fn parse_client_ip_invalid() {
        let result = parse_client_overlay_ip("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn parse_garage_ip_valid() {
        let ip = parse_garage_overlay_ip("fd00:moto:1::abcd:1234:5678:9abc").unwrap();
        assert!(ip.is_garage());
    }

    #[test]
    fn parse_garage_ip_invalid() {
        let result = parse_garage_overlay_ip("fd00:moto:2::1");
        assert!(result.is_err());
    }
}
