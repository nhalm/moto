//! `WireGuard` `ConfigMap` and Secret management for garages.
//!
//! Per moto-club.md spec v1.3 step 7, this creates:
//! - `wireguard-config` `ConfigMap` (address, peers configuration)
//! - `wireguard-keys` Secret (`private_key`, `public_key`)
//!
//! The public key is returned so it can be stored in the database
//! for client session routing.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::core::v1::{ConfigMap, Secret};
use kube::api::{Api, ObjectMeta, PostParams};
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::Result;
use moto_wgtunnel_types::{OverlayIp, WgPrivateKey, WgPublicKey};

use crate::GarageK8s;

/// `WireGuard` config `ConfigMap` name.
pub const WIREGUARD_CONFIG_NAME: &str = "wireguard-config";

/// `WireGuard` keys Secret name.
pub const WIREGUARD_KEYS_SECRET_NAME: &str = "wireguard-keys";

/// Result of creating `WireGuard` resources.
#[derive(Debug, Clone)]
pub struct WireGuardResources {
    /// The garage's `WireGuard` public key (for client session routing).
    pub public_key: WgPublicKey,
    /// The garage's overlay IP address.
    pub overlay_ip: OverlayIp,
}

/// Trait for garage `WireGuard` resource operations.
pub trait GarageWireGuardOps {
    /// Creates `WireGuard` `ConfigMap` and Secret for a garage.
    ///
    /// Per moto-club.md spec v1.3 step 7:
    /// - Generate `WireGuard` keypair
    /// - Create `wireguard-config` `ConfigMap` (address, peers)
    /// - Create `wireguard-keys` Secret (`private_key`, `public_key`)
    /// - Return `public_key` for database storage
    ///
    /// The overlay IP is derived deterministically from the garage ID
    /// using the IPAM hash algorithm.
    ///
    /// # Errors
    ///
    /// Returns an error if `ConfigMap` or Secret creation fails.
    fn create_wireguard_resources(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<WireGuardResources>> + Send;
}

impl GarageWireGuardOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %id))]
    async fn create_wireguard_resources(&self, id: &GarageId) -> Result<WireGuardResources> {
        let namespace = format!("moto-garage-{}", id.short());

        // Generate WireGuard keypair
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();

        // Derive overlay IP deterministically from garage ID
        // Uses same algorithm as IPAM (hash-based)
        let overlay_ip = derive_garage_overlay_ip(&id.to_string());

        debug!(
            namespace = %namespace,
            public_key = %public_key,
            overlay_ip = %overlay_ip,
            "creating WireGuard resources"
        );

        // Create wireguard-config ConfigMap
        let config_map = build_wireguard_config(&namespace, &overlay_ip);
        let configmap_api: Api<ConfigMap> =
            Api::namespaced(self.client.inner().clone(), &namespace);
        configmap_api
            .create(&PostParams::default(), &config_map)
            .await
            .map_err(moto_k8s::Error::ConfigMapCreate)?;

        debug!(namespace = %namespace, "wireguard-config ConfigMap created");

        // Create wireguard-keys Secret
        let secret = build_wireguard_keys_secret(&namespace, &private_key, &public_key);
        let secret_api: Api<Secret> = Api::namespaced(self.client.inner().clone(), &namespace);
        secret_api
            .create(&PostParams::default(), &secret)
            .await
            .map_err(moto_k8s::Error::SecretCreate)?;

        debug!(namespace = %namespace, "wireguard-keys Secret created");

        Ok(WireGuardResources {
            public_key,
            overlay_ip,
        })
    }
}

/// Derives a garage overlay IP deterministically from the garage ID.
///
/// Uses the same algorithm as `moto_club_wg::ipam::hash_garage_id`.
fn derive_garage_overlay_ip(garage_id: &str) -> OverlayIp {
    use std::hash::{Hash, Hasher};

    // FNV-1a hash for stability across runs (matches IPAM implementation)
    struct StableHasher {
        state: u64,
    }

    impl StableHasher {
        const fn new() -> Self {
            Self {
                state: 0x517c_c1b7_2722_0a95, // FNV-1a offset basis (64-bit)
            }
        }
    }

    impl Hasher for StableHasher {
        fn write(&mut self, bytes: &[u8]) {
            const PRIME: u64 = 0x0000_0100_0000_01b3;
            for &byte in bytes {
                self.state ^= u64::from(byte);
                self.state = self.state.wrapping_mul(PRIME);
            }
        }

        fn finish(&self) -> u64 {
            self.state
        }
    }

    let mut hasher = StableHasher::new();
    garage_id.hash(&mut hasher);
    let hash = hasher.finish();

    // Use lower 48 bits to fit in IPv6 host portion
    // Avoid 0 (network address) by ensuring at least 1
    let host_id = hash & 0x0000_FFFF_FFFF_FFFF;
    let host_id = if host_id == 0 { 1 } else { host_id };

    OverlayIp::garage(host_id)
}

/// Builds `WireGuard` config `ConfigMap`.
///
/// Contains the `WireGuard` interface configuration:
/// - Address: The garage's overlay IP
/// - Peers: Empty initially (populated dynamically by garage daemon)
fn build_wireguard_config(namespace: &str, overlay_ip: &OverlayIp) -> ConfigMap {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "wireguard".to_string());
    labels.insert("moto.dev/component".to_string(), "wireguard".to_string());

    // WireGuard config format
    // Address is the garage's overlay IP with /128 prefix
    let wg_config = format!(
        r"[Interface]
Address = {overlay_ip}/128
ListenPort = 51820

# Peers are managed dynamically by the garage daemon
# by polling moto-club's peer list endpoint
"
    );

    let mut data = BTreeMap::new();
    data.insert("wg0.conf".to_string(), wg_config);

    ConfigMap {
        metadata: ObjectMeta {
            name: Some(WIREGUARD_CONFIG_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    }
}

/// Builds `WireGuard` keys Secret.
///
/// Contains:
/// - `private_key`: Base64-encoded `WireGuard` private key
/// - `public_key`: Base64-encoded `WireGuard` public key
fn build_wireguard_keys_secret(
    namespace: &str,
    private_key: &WgPrivateKey,
    public_key: &WgPublicKey,
) -> Secret {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "wireguard".to_string());
    labels.insert("moto.dev/component".to_string(), "wireguard".to_string());

    let mut string_data = BTreeMap::new();
    string_data.insert("private_key".to_string(), private_key.to_base64());
    string_data.insert("public_key".to_string(), public_key.to_base64());

    Secret {
        metadata: ObjectMeta {
            name: Some(WIREGUARD_KEYS_SECRET_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        type_: Some("Opaque".to_string()),
        string_data: Some(string_data),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_garage_overlay_ip_is_deterministic() {
        let ip1 = derive_garage_overlay_ip("garage-123");
        let ip2 = derive_garage_overlay_ip("garage-123");
        assert_eq!(ip1, ip2);
    }

    #[test]
    fn derive_garage_overlay_ip_different_ids_different_ips() {
        let ip1 = derive_garage_overlay_ip("garage-123");
        let ip2 = derive_garage_overlay_ip("garage-456");
        assert_ne!(ip1, ip2);
    }

    #[test]
    fn derive_garage_overlay_ip_is_garage_subnet() {
        let ip = derive_garage_overlay_ip("test-garage");
        assert!(ip.is_garage());
        assert!(!ip.is_client());
    }

    #[test]
    fn build_wireguard_config_structure() {
        let overlay_ip = derive_garage_overlay_ip("test-garage");
        let config = build_wireguard_config("moto-garage-abc12345", &overlay_ip);

        // Check metadata
        assert_eq!(
            config.metadata.name,
            Some(WIREGUARD_CONFIG_NAME.to_string())
        );
        assert_eq!(
            config.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check labels
        let labels = config.metadata.labels.as_ref().unwrap();
        assert_eq!(labels.get("app"), Some(&"wireguard".to_string()));
        assert_eq!(
            labels.get("moto.dev/component"),
            Some(&"wireguard".to_string())
        );

        // Check data
        let data = config.data.as_ref().unwrap();
        let wg_config = data.get("wg0.conf").unwrap();
        assert!(wg_config.contains("[Interface]"));
        assert!(wg_config.contains(&format!("Address = {overlay_ip}/128")));
        assert!(wg_config.contains("ListenPort = 51820"));
    }

    #[test]
    fn build_wireguard_keys_secret_structure() {
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();
        let secret = build_wireguard_keys_secret("moto-garage-abc12345", &private_key, &public_key);

        // Check metadata
        assert_eq!(
            secret.metadata.name,
            Some(WIREGUARD_KEYS_SECRET_NAME.to_string())
        );
        assert_eq!(
            secret.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );
        assert_eq!(secret.type_, Some("Opaque".to_string()));

        // Check labels
        let labels = secret.metadata.labels.as_ref().unwrap();
        assert_eq!(labels.get("app"), Some(&"wireguard".to_string()));
        assert_eq!(
            labels.get("moto.dev/component"),
            Some(&"wireguard".to_string())
        );

        // Check string_data
        let data = secret.string_data.as_ref().unwrap();
        assert_eq!(data.get("private_key"), Some(&private_key.to_base64()));
        assert_eq!(data.get("public_key"), Some(&public_key.to_base64()));
    }

    #[test]
    fn wireguard_resources_contains_public_key_and_ip() {
        let private_key = WgPrivateKey::generate();
        let public_key = private_key.public_key();
        let overlay_ip = derive_garage_overlay_ip("test-garage");

        let resources = WireGuardResources {
            public_key: public_key.clone(),
            overlay_ip,
        };

        assert_eq!(resources.public_key, public_key);
        assert!(resources.overlay_ip.is_garage());
    }
}
