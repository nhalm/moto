//! WebSocket handler for log streaming.
//!
//! Streams garage container logs to CLI clients over WebSocket.
//!
//! # Endpoint
//!
//! `WS /ws/v1/garages/{name}/logs?tail=100&follow=false&since=5m`
//!
//! # Protocol
//!
//! Server sends JSON messages with `type` discriminator:
//! - `log`: A log line with timestamp
//! - `error`: An error message
//! - `eof`: End of log stream with reason

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use moto_k8s::{LogStream, PodLogOptions};

/// Query parameters for log streaming.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct LogStreamQuery {
    /// Number of historical lines to send first (default: 100).
    #[serde(default = "default_tail")]
    pub tail: i64,
    /// Stream new lines after history (default: false).
    #[serde(default)]
    pub follow: bool,
    /// Relative duration (e.g., `5m`, `1h`).
    pub since: Option<String>,
}

const fn default_tail() -> i64 {
    100
}

/// Message types sent over the log WebSocket.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum LogMessage {
    /// A log line.
    #[serde(rename = "log")]
    Log {
        /// ISO 8601 timestamp.
        timestamp: String,
        /// The log line content.
        line: String,
    },
    /// An error.
    #[serde(rename = "error")]
    Error {
        /// Error description.
        message: String,
    },
    /// End of stream.
    #[serde(rename = "eof")]
    Eof {
        /// Reason for stream end (e.g., `complete` or `pod_terminated`).
        reason: String,
    },
}

/// Trait for providing log streaming context.
///
/// This trait abstracts the application state needed by the log WebSocket handler.
pub trait LogStreamingContext: Clone + Send + Sync + 'static {
    /// Look up a garage by name and owner. Returns namespace and status.
    ///
    /// # Errors
    ///
    /// Returns a `LogStreamError` if the garage is not found, not owned, etc.
    fn resolve_garage(
        &self,
        name: &str,
        owner: &str,
    ) -> impl std::future::Future<Output = Result<GarageInfo, LogStreamError>> + Send;

    /// Open a K8s pod log stream for the given namespace.
    ///
    /// # Errors
    ///
    /// Returns a `LogStreamError` if the log stream cannot be opened.
    fn stream_pod_logs(
        &self,
        namespace: &str,
        options: &PodLogOptions,
    ) -> impl std::future::Future<Output = Result<LogStream, LogStreamError>> + Send;
}

/// Information about a garage needed for log streaming.
#[derive(Debug, Clone)]
pub struct GarageInfo {
    /// K8s namespace for the garage.
    pub namespace: String,
    /// Current garage status as a string (e.g., "pending", "ready").
    pub status: String,
}

/// Errors that can occur during log streaming setup.
#[derive(Debug)]
pub enum LogStreamError {
    /// Garage not found.
    NotFound(String),
    /// Garage not owned by the requesting user.
    NotOwned(String),
    /// Garage is in a state that doesn't support log streaming.
    InvalidState(String),
    /// K8s error (pod not found, log stream failed, etc.).
    Kubernetes(String),
    /// Internal error.
    Internal(String),
}

impl LogStreamError {
    /// Extract the inner message from any variant.
    fn into_message(self) -> String {
        match self {
            Self::NotFound(m)
            | Self::NotOwned(m)
            | Self::InvalidState(m)
            | Self::Kubernetes(m)
            | Self::Internal(m) => m,
        }
    }
}

/// Parse a "since" duration string (e.g., `5m`, `1h`, `30s`) to seconds.
fn parse_since_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (num_str, unit) = if let Some(n) = s.strip_suffix('s') {
        (n, 1i64)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60i64)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3600i64)
    } else {
        return s.parse::<i64>().ok();
    };

    num_str.parse::<i64>().ok().map(|n| n * unit)
}

/// Send an error message and close the WebSocket.
async fn send_error_and_close(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: String,
) {
    let error_msg = LogMessage::Error { message };
    if let Ok(json) = serde_json::to_string(&error_msg) {
        let _ = sender.send(Message::Text(json.into())).await;
    }
    let _ = sender.close().await;
}

/// Handle a WebSocket connection for log streaming.
///
/// This function validates the garage state, opens a K8s pod log stream,
/// and forwards log lines to the WebSocket client.
#[allow(clippy::too_many_lines)]
pub async fn handle_log_socket<C: LogStreamingContext>(
    socket: WebSocket,
    garage_name: String,
    owner: String,
    query: LogStreamQuery,
    context: C,
) {
    let (mut sender, mut receiver) = socket.split();

    // Resolve garage and validate state
    let garage_info = match context.resolve_garage(&garage_name, &owner).await {
        Ok(info) => info,
        Err(e) => {
            send_error_and_close(&mut sender, e.into_message()).await;
            return;
        }
    };

    // Check garage state per spec: reject Pending and Terminated
    match garage_info.status.as_str() {
        "pending" => {
            send_error_and_close(&mut sender, "garage not ready".to_string()).await;
            return;
        }
        "terminated" => {
            send_error_and_close(&mut sender, "garage terminated".to_string()).await;
            return;
        }
        // initializing, ready, failed — all allowed
        _ => {}
    }

    // Build K8s log options
    let since_seconds = query.since.as_deref().and_then(parse_since_duration);
    let options = PodLogOptions {
        tail_lines: Some(query.tail),
        since_seconds,
        follow: query.follow,
    };

    // Open pod log stream
    let log_stream = match context
        .stream_pod_logs(&garage_info.namespace, &options)
        .await
    {
        Ok(stream) => stream,
        Err(e) => {
            send_error_and_close(&mut sender, e.into_message()).await;
            return;
        }
    };

    tracing::info!(garage = %garage_name, follow = query.follow, tail = query.tail, "log WebSocket connected");

    stream_logs(
        &mut sender,
        &mut receiver,
        log_stream,
        &garage_name,
        query.follow,
    )
    .await;

    tracing::info!(garage = %garage_name, "log WebSocket disconnected");
}

/// Stream log lines from a K8s log stream to the WebSocket client.
async fn stream_logs(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    receiver: &mut futures_util::stream::SplitStream<WebSocket>,
    log_stream: LogStream,
    garage_name: &str,
    follow: bool,
) {
    let mut log_stream = std::pin::pin!(log_stream);

    loop {
        tokio::select! {
            line_result = log_stream.next() => {
                match line_result {
                    Some(Ok(line)) => {
                        let log_msg = LogMessage::Log {
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            line: line.trim_end().to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&log_msg)
                            && sender.send(Message::Text(json.into())).await.is_err()
                        {
                            tracing::debug!(garage = %garage_name, "log WebSocket send failed, closing");
                            break;
                        }
                    }
                    Some(Err(e)) => {
                        tracing::debug!(garage = %garage_name, error = %e, "log stream error");
                        let error_msg = LogMessage::Error {
                            message: format!("Pod log error: {e}"),
                        };
                        if let Ok(json) = serde_json::to_string(&error_msg) {
                            let _ = sender.send(Message::Text(json.into())).await;
                        }
                        break;
                    }
                    None => {
                        let reason = if follow { "pod_terminated" } else { "complete" };
                        let eof_msg = LogMessage::Eof {
                            reason: reason.to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&eof_msg) {
                            let _ = sender.send(Message::Text(json.into())).await;
                        }
                        break;
                    }
                }
            }
            result = receiver.next() => {
                match result {
                    Some(Ok(Message::Ping(data))) => {
                        if sender.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!(garage = %garage_name, "log WebSocket closed by client");
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::debug!(garage = %garage_name, error = %e, "WebSocket error");
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_since_duration_minutes() {
        assert_eq!(parse_since_duration("5m"), Some(300));
    }

    #[test]
    fn parse_since_duration_hours() {
        assert_eq!(parse_since_duration("1h"), Some(3600));
    }

    #[test]
    fn parse_since_duration_seconds() {
        assert_eq!(parse_since_duration("30s"), Some(30));
    }

    #[test]
    fn parse_since_duration_plain_number() {
        assert_eq!(parse_since_duration("120"), Some(120));
    }

    #[test]
    fn parse_since_duration_empty() {
        assert_eq!(parse_since_duration(""), None);
    }

    #[test]
    fn parse_since_duration_invalid() {
        assert_eq!(parse_since_duration("abc"), None);
    }

    #[test]
    fn log_message_serialization() {
        let msg = LogMessage::Log {
            timestamp: "2026-01-21T10:15:32Z".to_string(),
            line: "Starting dev environment...".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"log""#));
        assert!(json.contains(r#""line":"Starting dev environment...""#));
    }

    #[test]
    fn error_message_serialization() {
        let msg = LogMessage::Error {
            message: "Pod not found".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""message":"Pod not found""#));
    }

    #[test]
    fn eof_message_serialization() {
        let msg = LogMessage::Eof {
            reason: "pod_terminated".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""type":"eof""#));
        assert!(json.contains(r#""reason":"pod_terminated""#));
    }

    #[test]
    fn log_stream_query_defaults() {
        let query: LogStreamQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(query.tail, 100);
        assert!(!query.follow);
        assert!(query.since.is_none());
    }
}
