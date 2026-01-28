//! Kubernetes Deployment and Service operations for bikes.

use std::collections::BTreeMap;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, HTTPGetAction, PodSpec, PodTemplateSpec, Probe, ResourceRequirements,
    Service, ServicePort, ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, ObjectMeta, Patch, PatchParams, PostParams};
use tracing::{debug, instrument};

use crate::{Error, K8sClient, Result};

/// Configuration for creating a bike deployment.
#[derive(Debug, Clone)]
pub struct BikeDeploymentConfig {
    /// Name of the bike (used for deployment and service names).
    pub name: String,
    /// Container image to deploy.
    pub image: String,
    /// Number of replicas.
    pub replicas: u32,
    /// Main API port.
    pub port: u16,
    /// Health check port.
    pub health_port: u16,
    /// Health check path for readiness probe.
    pub health_path: String,
    /// CPU request (e.g., "500m").
    pub cpu_request: Option<String>,
    /// CPU limit (e.g., "2").
    pub cpu_limit: Option<String>,
    /// Memory request (e.g., "512Mi").
    pub memory_request: Option<String>,
    /// Memory limit (e.g., "2Gi").
    pub memory_limit: Option<String>,
}

/// Trait for deployment operations.
pub trait DeploymentOps {
    /// Creates or updates a bike deployment in the specified namespace.
    ///
    /// If the deployment already exists, it will be updated with the new config.
    fn deploy_bike(
        &self,
        namespace: &str,
        config: &BikeDeploymentConfig,
    ) -> impl std::future::Future<Output = Result<Deployment>> + Send;

    /// Gets a deployment by name in the specified namespace.
    fn get_deployment(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl std::future::Future<Output = Result<Deployment>> + Send;

    /// Checks if a deployment exists in the specified namespace.
    fn deployment_exists(
        &self,
        namespace: &str,
        name: &str,
    ) -> impl std::future::Future<Output = Result<bool>> + Send;
}

impl DeploymentOps for K8sClient {
    #[instrument(skip(self, config), fields(deployment = %config.name, namespace = %namespace))]
    async fn deploy_bike(
        &self,
        namespace: &str,
        config: &BikeDeploymentConfig,
    ) -> Result<Deployment> {
        let deployment_name = format!("moto-{}", config.name);

        // Check if deployment exists
        let exists = self.deployment_exists(namespace, &deployment_name).await?;

        let deployment = build_deployment(config);
        let service = build_service(config);

        // Create or update deployment
        let deployment_api: Api<Deployment> = Api::namespaced(self.inner().clone(), namespace);
        let result = if exists {
            debug!("updating existing deployment");
            let patch = Patch::Apply(&deployment);
            deployment_api
                .patch(
                    &deployment_name,
                    &PatchParams::apply("moto-cli").force(),
                    &patch,
                )
                .await
                .map_err(Error::DeploymentUpdate)?
        } else {
            debug!("creating new deployment");
            deployment_api
                .create(&PostParams::default(), &deployment)
                .await
                .map_err(Error::DeploymentCreate)?
        };

        // Create or update service
        let service_api: Api<Service> = Api::namespaced(self.inner().clone(), namespace);
        let service_name = format!("moto-{}", config.name);
        let service_exists = service_api.get(&service_name).await.is_ok();

        if service_exists {
            debug!("updating existing service");
            let patch = Patch::Apply(&service);
            service_api
                .patch(
                    &service_name,
                    &PatchParams::apply("moto-cli").force(),
                    &patch,
                )
                .await
                .map_err(Error::ServiceCreate)?;
        } else {
            debug!("creating new service");
            service_api
                .create(&PostParams::default(), &service)
                .await
                .map_err(Error::ServiceCreate)?;
        }

        Ok(result)
    }

    #[instrument(skip(self), fields(deployment = %name, namespace = %namespace))]
    async fn get_deployment(&self, namespace: &str, name: &str) -> Result<Deployment> {
        let api: Api<Deployment> = Api::namespaced(self.inner().clone(), namespace);

        debug!("getting deployment");
        api.get(name).await.map_err(|e| {
            if is_not_found(&e) {
                Error::DeploymentNotFound(name.to_string())
            } else {
                Error::DeploymentGet(e)
            }
        })
    }

    #[instrument(skip(self), fields(deployment = %name, namespace = %namespace))]
    async fn deployment_exists(&self, namespace: &str, name: &str) -> Result<bool> {
        let api: Api<Deployment> = Api::namespaced(self.inner().clone(), namespace);

        match api.get(name).await {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(Error::DeploymentGet(e)),
        }
    }
}

/// Checks if a kube error is a "not found" error.
const fn is_not_found(e: &kube::Error) -> bool {
    matches!(
        e,
        kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })
    )
}

/// Builds a K8s Deployment from the bike config.
fn build_deployment(config: &BikeDeploymentConfig) -> Deployment {
    let deployment_name = format!("moto-{}", config.name);
    let app_label = deployment_name.clone();

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), app_label.clone());
    labels.insert("moto.dev/type".to_string(), "bike".to_string());
    labels.insert("moto.dev/name".to_string(), config.name.clone());

    // Build resource requirements
    let resources = build_resources(config);

    // Build probes
    let liveness_probe = build_probe(
        &config.health_path.replace("/ready", "/live"),
        config.health_port,
        10,
    );
    let readiness_probe = build_probe(&config.health_path, config.health_port, 5);
    let startup_probe = build_startup_probe(
        &config.health_path.replace("/ready", "/startup"),
        config.health_port,
    );

    Deployment {
        metadata: ObjectMeta {
            name: Some(deployment_name),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(i32::try_from(config.replicas).unwrap_or(i32::MAX)),
            selector: LabelSelector {
                match_labels: Some({
                    let mut selector = BTreeMap::new();
                    selector.insert("app".to_string(), app_label);
                    selector
                }),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: config.name.clone(),
                        image: Some(config.image.clone()),
                        ports: Some(vec![
                            ContainerPort {
                                container_port: i32::from(config.port),
                                name: Some("api".to_string()),
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: i32::from(config.health_port),
                                name: Some("health".to_string()),
                                ..Default::default()
                            },
                            ContainerPort {
                                container_port: 9090,
                                name: Some("metrics".to_string()),
                                ..Default::default()
                            },
                        ]),
                        resources: Some(resources),
                        liveness_probe: Some(liveness_probe),
                        readiness_probe: Some(readiness_probe),
                        startup_probe: Some(startup_probe),
                        ..Default::default()
                    }],
                    security_context: Some(k8s_openapi::api::core::v1::PodSecurityContext {
                        run_as_user: Some(1000),
                        run_as_group: Some(1000),
                        run_as_non_root: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Builds a K8s Service from the bike config.
fn build_service(config: &BikeDeploymentConfig) -> Service {
    let service_name = format!("moto-{}", config.name);
    let app_label = service_name.clone();

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), app_label.clone());
    labels.insert("moto.dev/type".to_string(), "bike".to_string());
    labels.insert("moto.dev/name".to_string(), config.name.clone());

    let mut selector = BTreeMap::new();
    selector.insert("app".to_string(), app_label);

    Service {
        metadata: ObjectMeta {
            name: Some(service_name),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![
                ServicePort {
                    name: Some("api".to_string()),
                    port: i32::from(config.port),
                    target_port: Some(IntOrString::Int(i32::from(config.port))),
                    ..Default::default()
                },
                ServicePort {
                    name: Some("health".to_string()),
                    port: i32::from(config.health_port),
                    target_port: Some(IntOrString::Int(i32::from(config.health_port))),
                    ..Default::default()
                },
                ServicePort {
                    name: Some("metrics".to_string()),
                    port: 9090,
                    target_port: Some(IntOrString::Int(9090)),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Builds resource requirements from the config.
fn build_resources(config: &BikeDeploymentConfig) -> ResourceRequirements {
    let mut requests = BTreeMap::new();
    let mut limits = BTreeMap::new();

    if let Some(ref cpu) = config.cpu_request {
        requests.insert("cpu".to_string(), Quantity(cpu.clone()));
    }
    if let Some(ref cpu) = config.cpu_limit {
        limits.insert("cpu".to_string(), Quantity(cpu.clone()));
    }
    if let Some(ref mem) = config.memory_request {
        requests.insert("memory".to_string(), Quantity(mem.clone()));
    }
    if let Some(ref mem) = config.memory_limit {
        limits.insert("memory".to_string(), Quantity(mem.clone()));
    }

    ResourceRequirements {
        requests: if requests.is_empty() {
            None
        } else {
            Some(requests)
        },
        limits: if limits.is_empty() {
            None
        } else {
            Some(limits)
        },
        ..Default::default()
    }
}

/// Builds a liveness/readiness probe.
fn build_probe(path: &str, port: u16, period_seconds: i32) -> Probe {
    Probe {
        http_get: Some(HTTPGetAction {
            path: Some(path.to_string()),
            port: IntOrString::Int(i32::from(port)),
            ..Default::default()
        }),
        period_seconds: Some(period_seconds),
        ..Default::default()
    }
}

/// Builds a startup probe with higher failure threshold.
fn build_startup_probe(path: &str, port: u16) -> Probe {
    Probe {
        http_get: Some(HTTPGetAction {
            path: Some(path.to_string()),
            port: IntOrString::Int(i32::from(port)),
            ..Default::default()
        }),
        failure_threshold: Some(30),
        period_seconds: Some(1),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> BikeDeploymentConfig {
        BikeDeploymentConfig {
            name: "club".to_string(),
            image: "moto-club:abc123".to_string(),
            replicas: 2,
            port: 8080,
            health_port: 8081,
            health_path: "/health/ready".to_string(),
            cpu_request: Some("500m".to_string()),
            cpu_limit: Some("2".to_string()),
            memory_request: Some("512Mi".to_string()),
            memory_limit: Some("2Gi".to_string()),
        }
    }

    #[test]
    fn test_build_deployment() {
        let config = test_config();
        let deployment = build_deployment(&config);

        assert_eq!(deployment.metadata.name, Some("moto-club".to_string()));

        let spec = deployment.spec.unwrap();
        assert_eq!(spec.replicas, Some(2));

        let template = spec.template;
        let pod_spec = template.spec.unwrap();
        assert_eq!(pod_spec.containers.len(), 1);

        let container = &pod_spec.containers[0];
        assert_eq!(container.name, "club");
        assert_eq!(container.image, Some("moto-club:abc123".to_string()));

        // Check ports
        let ports = container.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].container_port, 8080);
        assert_eq!(ports[1].container_port, 8081);
        assert_eq!(ports[2].container_port, 9090);

        // Check security context
        let security = pod_spec.security_context.unwrap();
        assert_eq!(security.run_as_user, Some(1000));
        assert_eq!(security.run_as_group, Some(1000));
        assert_eq!(security.run_as_non_root, Some(true));
    }

    #[test]
    fn test_build_service() {
        let config = test_config();
        let service = build_service(&config);

        assert_eq!(service.metadata.name, Some("moto-club".to_string()));

        let spec = service.spec.unwrap();
        let ports = spec.ports.unwrap();
        assert_eq!(ports.len(), 3);

        // Check port names and values
        assert_eq!(ports[0].name, Some("api".to_string()));
        assert_eq!(ports[0].port, 8080);
        assert_eq!(ports[1].name, Some("health".to_string()));
        assert_eq!(ports[1].port, 8081);
        assert_eq!(ports[2].name, Some("metrics".to_string()));
        assert_eq!(ports[2].port, 9090);
    }

    #[test]
    fn test_build_resources() {
        let config = test_config();
        let resources = build_resources(&config);

        let requests = resources.requests.unwrap();
        assert_eq!(requests.get("cpu"), Some(&Quantity("500m".to_string())));
        assert_eq!(requests.get("memory"), Some(&Quantity("512Mi".to_string())));

        let limits = resources.limits.unwrap();
        assert_eq!(limits.get("cpu"), Some(&Quantity("2".to_string())));
        assert_eq!(limits.get("memory"), Some(&Quantity("2Gi".to_string())));
    }

    #[test]
    fn test_build_resources_empty() {
        let config = BikeDeploymentConfig {
            name: "test".to_string(),
            image: "test:latest".to_string(),
            replicas: 1,
            port: 8080,
            health_port: 8081,
            health_path: "/health/ready".to_string(),
            cpu_request: None,
            cpu_limit: None,
            memory_request: None,
            memory_limit: None,
        };
        let resources = build_resources(&config);

        assert!(resources.requests.is_none());
        assert!(resources.limits.is_none());
    }

    #[test]
    fn test_build_probe() {
        let probe = build_probe("/health/ready", 8081, 5);

        let http_get = probe.http_get.unwrap();
        assert_eq!(http_get.path, Some("/health/ready".to_string()));
        assert_eq!(http_get.port, IntOrString::Int(8081));
        assert_eq!(probe.period_seconds, Some(5));
    }

    #[test]
    fn test_build_startup_probe() {
        let probe = build_startup_probe("/health/startup", 8081);

        let http_get = probe.http_get.unwrap();
        assert_eq!(http_get.path, Some("/health/startup".to_string()));
        assert_eq!(probe.failure_threshold, Some(30));
        assert_eq!(probe.period_seconds, Some(1));
    }
}
