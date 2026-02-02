//! Supporting services (`PostgreSQL`, Redis) for garages.
//!
//! Per supporting-services.md spec, these are per-garage, ephemeral services
//! provisioned on-demand via CLI flags.

use std::collections::BTreeMap;
use std::future::Future;

use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
use k8s_openapi::api::core::v1::{
    Container, ContainerPort, EmptyDirVolumeSource, EnvVar, EnvVarSource, ExecAction, PodSpec,
    PodTemplateSpec, Probe, ResourceRequirements, Secret, SecretKeySelector, Service, ServicePort,
    ServiceSpec, Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::{Api, ObjectMeta, PostParams};
use rand::Rng;
use tracing::{debug, instrument};

use moto_club_types::GarageId;
use moto_k8s::Result;

use crate::GarageK8s;

/// Postgres Deployment name.
pub const POSTGRES_DEPLOYMENT_NAME: &str = "postgres";
/// Postgres Service name.
pub const POSTGRES_SERVICE_NAME: &str = "postgres";
/// Postgres credentials Secret name.
pub const POSTGRES_CREDENTIALS_SECRET_NAME: &str = "postgres-credentials";
/// Postgres port.
pub const POSTGRES_PORT: i32 = 5432;
/// Postgres image.
const POSTGRES_IMAGE: &str = "postgres:16";
/// Postgres default database name.
const POSTGRES_DB: &str = "dev";
/// Postgres default user.
const POSTGRES_USER: &str = "dev";

/// Label for supporting services.
pub const SUPPORTING_SERVICE_LABEL: &str = "moto.dev/supporting-service";

/// Trait for Postgres supporting service operations.
pub trait GaragePostgresOps {
    /// Creates Postgres Deployment, Service, and credentials Secret.
    ///
    /// Per supporting-services.md spec, this creates:
    /// - Deployment with postgres:16 image, readiness probe, resource limits
    /// - Service exposing port 5432
    /// - Secret with credentials (password, username, database, host, port, url)
    ///
    /// # Errors
    ///
    /// Returns an error if any K8s resource creation fails.
    fn create_garage_postgres(&self, id: &GarageId) -> impl Future<Output = Result<()>> + Send;

    /// Checks if Postgres is deployed in the garage namespace.
    fn postgres_exists(&self, id: &GarageId) -> impl Future<Output = Result<bool>> + Send;
}

impl GaragePostgresOps for GarageK8s {
    #[instrument(skip(self), fields(garage_id = %id))]
    async fn create_garage_postgres(&self, id: &GarageId) -> Result<()> {
        let namespace = format!("moto-garage-{}", id.short());

        // Generate random password
        let password = generate_password();

        // Create credentials Secret first (Deployment references it)
        debug!(namespace = %namespace, "creating PostgreSQL credentials Secret");
        let secret = build_postgres_credentials_secret(&namespace, &password);
        let secret_api: Api<Secret> = Api::namespaced(self.client.inner().clone(), &namespace);
        secret_api
            .create(&PostParams::default(), &secret)
            .await
            .map_err(moto_k8s::Error::NamespaceCreate)?;

        // Create Deployment
        debug!(namespace = %namespace, "creating PostgreSQL Deployment");
        let deployment = build_postgres_deployment(&namespace);
        let deployment_api: Api<Deployment> =
            Api::namespaced(self.client.inner().clone(), &namespace);
        deployment_api
            .create(&PostParams::default(), &deployment)
            .await
            .map_err(moto_k8s::Error::DeploymentCreate)?;

        // Create Service
        debug!(namespace = %namespace, "creating PostgreSQL Service");
        let service = build_postgres_service(&namespace);
        let service_api: Api<Service> = Api::namespaced(self.client.inner().clone(), &namespace);
        service_api
            .create(&PostParams::default(), &service)
            .await
            .map_err(moto_k8s::Error::ServiceCreate)?;

        debug!(namespace = %namespace, "PostgreSQL supporting service created");
        Ok(())
    }

    #[instrument(skip(self), fields(garage_id = %id))]
    async fn postgres_exists(&self, id: &GarageId) -> Result<bool> {
        let namespace = format!("moto-garage-{}", id.short());
        let api: Api<Deployment> = Api::namespaced(self.client.inner().clone(), &namespace);

        match api.get(POSTGRES_DEPLOYMENT_NAME).await {
            Ok(_) => Ok(true),
            Err(kube::Error::Api(kube::core::ErrorResponse { code: 404, .. })) => Ok(false),
            Err(e) => Err(moto_k8s::Error::DeploymentGet(e)),
        }
    }
}

/// Generates a random 32-character alphanumeric password.
fn generate_password() -> String {
    const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    (0..32)
        .map(|_| {
            let idx = rng.random_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Builds Postgres credentials Secret per spec.
fn build_postgres_credentials_secret(namespace: &str, password: &str) -> Secret {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), POSTGRES_DEPLOYMENT_NAME.to_string());
    labels.insert(SUPPORTING_SERVICE_LABEL.to_string(), "true".to_string());

    let url = format!(
        "postgresql://{POSTGRES_USER}:{password}@{POSTGRES_SERVICE_NAME}:{POSTGRES_PORT}/{POSTGRES_DB}"
    );

    let mut string_data = BTreeMap::new();
    string_data.insert("password".to_string(), password.to_string());
    string_data.insert("username".to_string(), POSTGRES_USER.to_string());
    string_data.insert("database".to_string(), POSTGRES_DB.to_string());
    string_data.insert("host".to_string(), POSTGRES_SERVICE_NAME.to_string());
    string_data.insert("port".to_string(), POSTGRES_PORT.to_string());
    string_data.insert("url".to_string(), url);

    Secret {
        metadata: ObjectMeta {
            name: Some(POSTGRES_CREDENTIALS_SECRET_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        type_: Some("Opaque".to_string()),
        string_data: Some(string_data),
        ..Default::default()
    }
}

/// Builds Postgres Deployment per spec.
#[allow(clippy::too_many_lines)]
fn build_postgres_deployment(namespace: &str) -> Deployment {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), POSTGRES_DEPLOYMENT_NAME.to_string());
    labels.insert(SUPPORTING_SERVICE_LABEL.to_string(), "true".to_string());

    // Resource requirements per spec
    let mut requests = BTreeMap::new();
    requests.insert("cpu".to_string(), Quantity("50m".to_string()));
    requests.insert("memory".to_string(), Quantity("128Mi".to_string()));

    let mut limits = BTreeMap::new();
    limits.insert("cpu".to_string(), Quantity("500m".to_string()));
    limits.insert("memory".to_string(), Quantity("512Mi".to_string()));

    let resources = ResourceRequirements {
        requests: Some(requests),
        limits: Some(limits),
        ..Default::default()
    };

    // Environment variables per spec
    let env_vars = vec![
        EnvVar {
            name: "POSTGRES_USER".to_string(),
            value: Some(POSTGRES_USER.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_PASSWORD".to_string(),
            value_from: Some(EnvVarSource {
                secret_key_ref: Some(SecretKeySelector {
                    name: POSTGRES_CREDENTIALS_SECRET_NAME.to_string(),
                    key: "password".to_string(),
                    optional: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        EnvVar {
            name: "POSTGRES_DB".to_string(),
            value: Some(POSTGRES_DB.to_string()),
            ..Default::default()
        },
        EnvVar {
            name: "PGDATA".to_string(),
            value: Some("/var/lib/postgresql/data/pgdata".to_string()),
            ..Default::default()
        },
    ];

    // Readiness probe per spec: pg_isready -U dev
    let readiness_probe = Probe {
        exec: Some(ExecAction {
            command: Some(vec![
                "pg_isready".to_string(),
                "-U".to_string(),
                POSTGRES_USER.to_string(),
            ]),
        }),
        initial_delay_seconds: Some(5),
        period_seconds: Some(5),
        ..Default::default()
    };

    // Volume mounts
    let volume_mounts = vec![VolumeMount {
        name: "data".to_string(),
        mount_path: "/var/lib/postgresql/data".to_string(),
        ..Default::default()
    }];

    // Volumes - emptyDir per spec (ephemeral)
    let volumes = vec![Volume {
        name: "data".to_string(),
        empty_dir: Some(EmptyDirVolumeSource::default()),
        ..Default::default()
    }];

    let container = Container {
        name: POSTGRES_DEPLOYMENT_NAME.to_string(),
        image: Some(POSTGRES_IMAGE.to_string()),
        ports: Some(vec![ContainerPort {
            container_port: POSTGRES_PORT,
            ..Default::default()
        }]),
        env: Some(env_vars),
        resources: Some(resources),
        volume_mounts: Some(volume_mounts),
        readiness_probe: Some(readiness_probe),
        ..Default::default()
    };

    Deployment {
        metadata: ObjectMeta {
            name: Some(POSTGRES_DEPLOYMENT_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels.clone()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(1),
            selector: LabelSelector {
                match_labels: Some({
                    let mut selector = BTreeMap::new();
                    selector.insert("app".to_string(), POSTGRES_DEPLOYMENT_NAME.to_string());
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
                    containers: vec![container],
                    volumes: Some(volumes),
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Builds Postgres Service per spec.
fn build_postgres_service(namespace: &str) -> Service {
    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), POSTGRES_SERVICE_NAME.to_string());
    labels.insert(SUPPORTING_SERVICE_LABEL.to_string(), "true".to_string());

    let mut selector = BTreeMap::new();
    selector.insert("app".to_string(), POSTGRES_DEPLOYMENT_NAME.to_string());

    Service {
        metadata: ObjectMeta {
            name: Some(POSTGRES_SERVICE_NAME.to_string()),
            namespace: Some(namespace.to_string()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: Some(ServiceSpec {
            selector: Some(selector),
            ports: Some(vec![ServicePort {
                port: POSTGRES_PORT,
                target_port: Some(IntOrString::Int(POSTGRES_PORT)),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_password_length() {
        let password = generate_password();
        assert_eq!(password.len(), 32);
    }

    #[test]
    fn generate_password_alphanumeric() {
        let password = generate_password();
        assert!(password.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn generate_password_unique() {
        let p1 = generate_password();
        let p2 = generate_password();
        assert_ne!(p1, p2);
    }

    #[test]
    fn build_postgres_credentials_secret_structure() {
        let secret = build_postgres_credentials_secret("moto-garage-abc12345", "testpass123");

        // Check metadata
        assert_eq!(
            secret.metadata.name,
            Some(POSTGRES_CREDENTIALS_SECRET_NAME.to_string())
        );
        assert_eq!(
            secret.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );
        assert_eq!(secret.type_, Some("Opaque".to_string()));

        // Check labels
        let labels = secret.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get("app"),
            Some(&POSTGRES_DEPLOYMENT_NAME.to_string())
        );
        assert_eq!(
            labels.get(SUPPORTING_SERVICE_LABEL),
            Some(&"true".to_string())
        );

        // Check string_data
        let data = secret.string_data.as_ref().unwrap();
        assert_eq!(data.get("password"), Some(&"testpass123".to_string()));
        assert_eq!(data.get("username"), Some(&POSTGRES_USER.to_string()));
        assert_eq!(data.get("database"), Some(&POSTGRES_DB.to_string()));
        assert_eq!(data.get("host"), Some(&POSTGRES_SERVICE_NAME.to_string()));
        assert_eq!(data.get("port"), Some(&"5432".to_string()));
        assert_eq!(
            data.get("url"),
            Some(&"postgresql://dev:testpass123@postgres:5432/dev".to_string())
        );
    }

    #[test]
    fn build_postgres_deployment_structure() {
        let deployment = build_postgres_deployment("moto-garage-abc12345");

        // Check metadata
        assert_eq!(
            deployment.metadata.name,
            Some(POSTGRES_DEPLOYMENT_NAME.to_string())
        );
        assert_eq!(
            deployment.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check labels
        let labels = deployment.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get("app"),
            Some(&POSTGRES_DEPLOYMENT_NAME.to_string())
        );
        assert_eq!(
            labels.get(SUPPORTING_SERVICE_LABEL),
            Some(&"true".to_string())
        );

        // Check spec
        let spec = deployment.spec.as_ref().unwrap();
        assert_eq!(spec.replicas, Some(1));

        // Check pod template
        let pod_spec = spec.template.spec.as_ref().unwrap();
        assert_eq!(pod_spec.containers.len(), 1);

        let container = &pod_spec.containers[0];
        assert_eq!(container.name, POSTGRES_DEPLOYMENT_NAME);
        assert_eq!(container.image, Some(POSTGRES_IMAGE.to_string()));

        // Check ports
        let ports = container.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].container_port, POSTGRES_PORT);

        // Check env vars
        let env = container.env.as_ref().unwrap();
        let user_env = env.iter().find(|e| e.name == "POSTGRES_USER").unwrap();
        assert_eq!(user_env.value, Some(POSTGRES_USER.to_string()));

        let pass_env = env.iter().find(|e| e.name == "POSTGRES_PASSWORD").unwrap();
        let secret_ref = pass_env
            .value_from
            .as_ref()
            .unwrap()
            .secret_key_ref
            .as_ref()
            .unwrap();
        assert_eq!(secret_ref.name, POSTGRES_CREDENTIALS_SECRET_NAME);
        assert_eq!(secret_ref.key, "password");

        let db_env = env.iter().find(|e| e.name == "POSTGRES_DB").unwrap();
        assert_eq!(db_env.value, Some(POSTGRES_DB.to_string()));

        let pgdata_env = env.iter().find(|e| e.name == "PGDATA").unwrap();
        assert_eq!(
            pgdata_env.value,
            Some("/var/lib/postgresql/data/pgdata".to_string())
        );

        // Check resources
        let resources = container.resources.as_ref().unwrap();
        let requests = resources.requests.as_ref().unwrap();
        assert_eq!(requests.get("cpu"), Some(&Quantity("50m".to_string())));
        assert_eq!(requests.get("memory"), Some(&Quantity("128Mi".to_string())));

        let limits = resources.limits.as_ref().unwrap();
        assert_eq!(limits.get("cpu"), Some(&Quantity("500m".to_string())));
        assert_eq!(limits.get("memory"), Some(&Quantity("512Mi".to_string())));

        // Check readiness probe
        let probe = container.readiness_probe.as_ref().unwrap();
        let exec = probe.exec.as_ref().unwrap();
        assert_eq!(
            exec.command,
            Some(vec![
                "pg_isready".to_string(),
                "-U".to_string(),
                POSTGRES_USER.to_string()
            ])
        );
        assert_eq!(probe.initial_delay_seconds, Some(5));
        assert_eq!(probe.period_seconds, Some(5));

        // Check volumes
        let volumes = pod_spec.volumes.as_ref().unwrap();
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].name, "data");
        assert!(volumes[0].empty_dir.is_some());

        // Check volume mounts
        let mounts = container.volume_mounts.as_ref().unwrap();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].name, "data");
        assert_eq!(mounts[0].mount_path, "/var/lib/postgresql/data");
    }

    #[test]
    fn build_postgres_service_structure() {
        let service = build_postgres_service("moto-garage-abc12345");

        // Check metadata
        assert_eq!(
            service.metadata.name,
            Some(POSTGRES_SERVICE_NAME.to_string())
        );
        assert_eq!(
            service.metadata.namespace,
            Some("moto-garage-abc12345".to_string())
        );

        // Check labels
        let labels = service.metadata.labels.as_ref().unwrap();
        assert_eq!(labels.get("app"), Some(&POSTGRES_SERVICE_NAME.to_string()));
        assert_eq!(
            labels.get(SUPPORTING_SERVICE_LABEL),
            Some(&"true".to_string())
        );

        // Check spec
        let spec = service.spec.as_ref().unwrap();

        // Check selector
        let selector = spec.selector.as_ref().unwrap();
        assert_eq!(
            selector.get("app"),
            Some(&POSTGRES_DEPLOYMENT_NAME.to_string())
        );

        // Check ports
        let ports = spec.ports.as_ref().unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].port, POSTGRES_PORT);
        assert_eq!(ports[0].target_port, Some(IntOrString::Int(POSTGRES_PORT)));
    }
}
