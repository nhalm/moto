//! Garage `NetworkPolicy` management.
//!
//! Creates the garage-isolation `NetworkPolicy` per garage-isolation.md spec:
//! - Deny all ingress (`WireGuard` tunnel bypasses at pod level)
//! - Allow egress: DNS, keybox, same-namespace (postgres/redis), internet
//! - Block: cluster internal networks, cloud metadata, `WireGuard` range

use std::future::Future;

use k8s_openapi::api::networking::v1::{
    IPBlock, NetworkPolicy, NetworkPolicyEgressRule, NetworkPolicyPeer, NetworkPolicyPort,
    NetworkPolicySpec,
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::ObjectMeta;
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::{NetworkPolicyOps, Result};

use crate::GarageK8s;

/// Name of the garage isolation `NetworkPolicy`.
pub const GARAGE_ISOLATION_POLICY_NAME: &str = "garage-isolation";

/// Trait for garage `NetworkPolicy` operations.
pub trait GarageNetworkPolicyOps {
    /// Creates the garage-isolation `NetworkPolicy` in the garage namespace.
    ///
    /// The policy:
    /// - Applies to all pods in the namespace
    /// - Denies all ingress
    /// - Allows egress to: DNS, keybox, same-namespace (postgres/redis), internet
    /// - Blocks: cluster internal networks, cloud metadata, `WireGuard` range
    ///
    /// # Errors
    ///
    /// Returns an error if the `NetworkPolicy` already exists or creation fails.
    fn create_garage_network_policy(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<NetworkPolicy>> + Send;

    /// Checks if the garage-isolation `NetworkPolicy` exists.
    fn garage_network_policy_exists(
        &self,
        id: &GarageId,
    ) -> impl Future<Output = Result<bool>> + Send;
}

impl GarageNetworkPolicyOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %id))]
    async fn create_garage_network_policy(&self, id: &GarageId) -> Result<NetworkPolicy> {
        let namespace = format!("moto-garage-{}", id.short());

        debug!(namespace = %namespace, "creating garage-isolation NetworkPolicy");

        let policy = build_garage_isolation_policy(&namespace);

        self.client()
            .create_network_policy(&namespace, &policy)
            .await
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn garage_network_policy_exists(&self, id: &GarageId) -> Result<bool> {
        let namespace = format!("moto-garage-{}", id.short());
        self.client()
            .network_policy_exists(&namespace, GARAGE_ISOLATION_POLICY_NAME)
            .await
    }
}

/// Builds the garage-isolation `NetworkPolicy` per garage-isolation.md spec.
fn build_garage_isolation_policy(namespace: &str) -> NetworkPolicy {
    NetworkPolicy {
        metadata: ObjectMeta {
            name: Some(GARAGE_ISOLATION_POLICY_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(NetworkPolicySpec {
            // Applies to all pods in namespace
            pod_selector: LabelSelector::default(),
            policy_types: Some(vec!["Ingress".to_string(), "Egress".to_string()]),
            // Deny all ingress (WireGuard tunnel bypasses at pod level)
            ingress: Some(vec![]),
            egress: Some(build_egress_rules()),
        }),
    }
}

/// Builds the egress rules per garage-isolation.md spec.
fn build_egress_rules() -> Vec<NetworkPolicyEgressRule> {
    vec![
        // Rule 1: Allow DNS to kube-system
        build_dns_egress_rule(),
        // Rule 2: Allow keybox in moto-system namespace
        build_keybox_egress_rule(),
        // Rule 3: Allow same-namespace traffic (supporting services: postgres, redis)
        build_same_namespace_egress_rule(),
        // Rule 4: Allow internet (anything not in cluster)
        build_internet_egress_rule(),
    ]
}

/// DNS egress rule: allows UDP 53 to kube-system namespace.
fn build_dns_egress_rule() -> NetworkPolicyEgressRule {
    NetworkPolicyEgressRule {
        to: Some(vec![NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector {
                match_labels: Some(
                    std::iter::once((
                        "kubernetes.io/metadata.name".to_string(),
                        "kube-system".to_string(),
                    ))
                    .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ports: Some(vec![NetworkPolicyPort {
            protocol: Some("UDP".to_string()),
            port: Some(IntOrString::Int(53)),
            ..Default::default()
        }]),
    }
}

/// Keybox egress rule: allows TCP 8080 to keybox in moto-system namespace.
fn build_keybox_egress_rule() -> NetworkPolicyEgressRule {
    NetworkPolicyEgressRule {
        to: Some(vec![NetworkPolicyPeer {
            namespace_selector: Some(LabelSelector {
                match_labels: Some(
                    std::iter::once(("moto.dev/type".to_string(), "system".to_string())).collect(),
                ),
                ..Default::default()
            }),
            pod_selector: Some(LabelSelector {
                match_labels: Some(
                    std::iter::once((
                        "app.kubernetes.io/component".to_string(),
                        "moto-keybox".to_string(),
                    ))
                    .collect(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        }]),
        ports: Some(vec![NetworkPolicyPort {
            protocol: Some("TCP".to_string()),
            port: Some(IntOrString::Int(8080)),
            ..Default::default()
        }]),
    }
}

/// Same-namespace egress rule: allows postgres (5432) and redis (6379) to pods in same namespace.
fn build_same_namespace_egress_rule() -> NetworkPolicyEgressRule {
    NetworkPolicyEgressRule {
        to: Some(vec![NetworkPolicyPeer {
            // Empty pod_selector matches all pods in same namespace
            pod_selector: Some(LabelSelector::default()),
            ..Default::default()
        }]),
        ports: Some(vec![
            NetworkPolicyPort {
                protocol: Some("TCP".to_string()),
                port: Some(IntOrString::Int(5432)), // postgres
                ..Default::default()
            },
            NetworkPolicyPort {
                protocol: Some("TCP".to_string()),
                port: Some(IntOrString::Int(6379)), // redis
                ..Default::default()
            },
        ]),
    }
}

/// Internet egress rule: allows 0.0.0.0/0 except internal/reserved ranges.
fn build_internet_egress_rule() -> NetworkPolicyEgressRule {
    NetworkPolicyEgressRule {
        to: Some(vec![NetworkPolicyPeer {
            ip_block: Some(IPBlock {
                cidr: "0.0.0.0/0".to_string(),
                except: Some(vec![
                    "10.0.0.0/8".to_string(),     // Private (cluster internal)
                    "172.16.0.0/12".to_string(),  // Private (cluster internal)
                    "192.168.0.0/16".to_string(), // Private (cluster internal)
                    "100.64.0.0/10".to_string(),  // CGNAT / WireGuard range
                    "169.254.0.0/16".to_string(), // Link-local / cloud metadata
                    "127.0.0.0/8".to_string(),    // Loopback
                ]),
            }),
            ..Default::default()
        }]),
        ports: None, // Allow all ports to internet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_garage_isolation_policy_structure() {
        let policy = build_garage_isolation_policy("moto-garage-abc12345");

        // Check metadata
        assert_eq!(
            policy.metadata.name,
            Some(GARAGE_ISOLATION_POLICY_NAME.to_string())
        );
        assert_eq!(
            policy.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check spec
        let spec = policy.spec.as_ref().unwrap();

        // podSelector should be empty (applies to all pods)
        assert!(spec.pod_selector.match_labels.is_none());
        assert!(spec.pod_selector.match_expressions.is_none());

        // policyTypes should include both Ingress and Egress
        assert_eq!(
            spec.policy_types,
            Some(vec!["Ingress".to_string(), "Egress".to_string()])
        );

        // Ingress should be empty (deny all)
        let ingress = spec.ingress.as_ref().unwrap();
        assert!(ingress.is_empty());

        // Egress should have 4 rules
        let egress = spec.egress.as_ref().unwrap();
        assert_eq!(egress.len(), 4);
    }

    #[test]
    fn dns_egress_rule_targets_kube_system() {
        let rule = build_dns_egress_rule();

        let peers = rule.to.as_ref().unwrap();
        assert_eq!(peers.len(), 1);

        let peer = &peers[0];
        let ns_selector = peer.namespace_selector.as_ref().unwrap();
        let match_labels = ns_selector.match_labels.as_ref().unwrap();
        assert_eq!(
            match_labels.get("kubernetes.io/metadata.name"),
            Some(&"kube-system".to_string())
        );

        let ports = rule.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].protocol, Some("UDP".to_string()));
        assert_eq!(ports[0].port, Some(IntOrString::Int(53)));
    }

    #[test]
    fn keybox_egress_rule_targets_moto_system() {
        let rule = build_keybox_egress_rule();

        let peers = rule.to.as_ref().unwrap();
        assert_eq!(peers.len(), 1);

        let peer = &peers[0];

        // Check namespace selector
        let ns_selector = peer.namespace_selector.as_ref().unwrap();
        let ns_labels = ns_selector.match_labels.as_ref().unwrap();
        assert_eq!(ns_labels.get("moto.dev/type"), Some(&"system".to_string()));

        // Check pod selector
        let pod_selector = peer.pod_selector.as_ref().unwrap();
        let pod_labels = pod_selector.match_labels.as_ref().unwrap();
        assert_eq!(
            pod_labels.get("app.kubernetes.io/component"),
            Some(&"moto-keybox".to_string())
        );

        // Check port
        let ports = rule.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].protocol, Some("TCP".to_string()));
        assert_eq!(ports[0].port, Some(IntOrString::Int(8080)));
    }

    #[test]
    fn same_namespace_egress_rule_allows_postgres_redis() {
        let rule = build_same_namespace_egress_rule();

        let peers = rule.to.as_ref().unwrap();
        assert_eq!(peers.len(), 1);

        // Empty pod_selector targets all pods in same namespace
        let peer = &peers[0];
        let pod_selector = peer.pod_selector.as_ref().unwrap();
        assert!(pod_selector.match_labels.is_none());

        // Check ports
        let ports = rule.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 2);

        // Postgres
        assert_eq!(ports[0].protocol, Some("TCP".to_string()));
        assert_eq!(ports[0].port, Some(IntOrString::Int(5432)));

        // Redis
        assert_eq!(ports[1].protocol, Some("TCP".to_string()));
        assert_eq!(ports[1].port, Some(IntOrString::Int(6379)));
    }

    #[test]
    fn internet_egress_rule_blocks_internal_ranges() {
        let rule = build_internet_egress_rule();

        let peers = rule.to.as_ref().unwrap();
        assert_eq!(peers.len(), 1);

        let peer = &peers[0];
        let ip_block = peer.ip_block.as_ref().unwrap();

        // CIDR should be 0.0.0.0/0
        assert_eq!(ip_block.cidr, "0.0.0.0/0");

        // Check except list
        let except = ip_block.except.as_ref().unwrap();
        assert_eq!(except.len(), 6);
        assert!(except.contains(&"10.0.0.0/8".to_string()));
        assert!(except.contains(&"172.16.0.0/12".to_string()));
        assert!(except.contains(&"192.168.0.0/16".to_string()));
        assert!(except.contains(&"100.64.0.0/10".to_string())); // WireGuard range
        assert!(except.contains(&"169.254.0.0/16".to_string())); // Cloud metadata
        assert!(except.contains(&"127.0.0.0/8".to_string())); // Loopback

        // No port restriction for internet
        assert!(rule.ports.is_none());
    }
}
