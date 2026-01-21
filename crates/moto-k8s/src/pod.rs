//! Pod operations for K8s.

use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, ListParams, LogParams};
use tracing::{debug, instrument};

use crate::{Error, Result};

/// Options for retrieving pod logs.
#[derive(Debug, Clone, Default)]
pub struct PodLogOptions {
    /// Number of lines from the end of the logs to show.
    pub tail_lines: Option<i64>,
    /// Seconds to look back for logs (relative to now).
    pub since_seconds: Option<i64>,
    /// Whether to follow (stream) the logs.
    /// Note: Streaming is not yet implemented - this flag is ignored.
    pub follow: bool,
}

/// Trait for pod operations.
pub trait PodOps {
    /// Lists pods in a namespace.
    fn list_pods(
        &self,
        namespace: &str,
        label_selector: Option<&str>,
    ) -> impl std::future::Future<Output = Result<Vec<Pod>>> + Send;

    /// Gets logs from pods in a namespace.
    ///
    /// Returns logs from all pods matching the optional label selector.
    /// If no selector is provided, returns logs from all pods in the namespace.
    fn get_pod_logs(
        &self,
        namespace: &str,
        label_selector: Option<&str>,
        options: &PodLogOptions,
    ) -> impl std::future::Future<Output = Result<String>> + Send;
}

impl PodOps for crate::K8sClient {
    #[instrument(skip(self), fields(namespace = %namespace))]
    async fn list_pods(&self, namespace: &str, label_selector: Option<&str>) -> Result<Vec<Pod>> {
        let api: Api<Pod> = Api::namespaced(self.inner().clone(), namespace);

        let mut params = ListParams::default();
        if let Some(selector) = label_selector {
            params = params.labels(selector);
        }

        debug!("listing pods");
        let list = api.list(&params).await.map_err(Error::PodList)?;

        Ok(list.items)
    }

    #[instrument(skip(self), fields(namespace = %namespace))]
    async fn get_pod_logs(
        &self,
        namespace: &str,
        label_selector: Option<&str>,
        options: &PodLogOptions,
    ) -> Result<String> {
        let pods = self.list_pods(namespace, label_selector).await?;

        if pods.is_empty() {
            return Ok(String::new());
        }

        let api: Api<Pod> = Api::namespaced(self.inner().clone(), namespace);
        let mut all_logs = Vec::new();

        for pod in pods {
            let pod_name = pod.metadata.name.as_deref().unwrap_or("unknown");
            let params = build_log_params(options);

            debug!(pod = %pod_name, "fetching logs");
            match api.logs(pod_name, &params).await {
                Ok(logs) => {
                    if !logs.is_empty() {
                        // Prefix each line with pod name for multi-pod scenarios
                        for line in logs.lines() {
                            all_logs.push(format!("[{pod_name}] {line}"));
                        }
                    }
                }
                Err(e) => {
                    debug!(pod = %pod_name, error = %e, "failed to get logs");
                    // Continue with other pods
                }
            }
        }

        Ok(all_logs.join("\n"))
    }
}

/// Build `LogParams` from `PodLogOptions`.
fn build_log_params(options: &PodLogOptions) -> LogParams {
    let mut params = LogParams::default();

    if let Some(tail) = options.tail_lines {
        params.tail_lines = Some(tail);
    }

    if let Some(since) = options.since_seconds {
        params.since_seconds = Some(since);
    }

    params
}
