//! Secret management commands (set, get, list).
//!
//! These commands interact with a running keybox server to manage secrets.
//! Authentication is done via an SVID token, which can be provided via:
//! - `--token` flag
//! - `MOTO_KEYBOX_TOKEN` environment variable

use std::io::{self, BufRead, IsTerminal};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::cli::{GetArgs, ListArgs, SetArgs};
use crate::error::{CliError, Result};

// API response types

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SecretMetadataResponse {
    name: String,
    scope: String,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    instance_id: Option<String>,
    version: u32,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GetSecretResponse {
    #[serde(flatten)]
    metadata: SecretMetadataResponse,
    value: String,
}

#[derive(Debug, Deserialize)]
struct ListSecretsResponse {
    secrets: Vec<SecretMetadataResponse>,
}

#[derive(Debug, Serialize)]
struct SetSecretRequest {
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    service: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    code: String,
    message: String,
}

/// Run the `set` command.
pub async fn run_set(cmd: &SetArgs) -> Result<()> {
    // Get the secret value
    let value = if cmd.stdin || (cmd.value.is_none() && !io::stdin().is_terminal()) {
        // Read from stdin
        tracing::debug!("Reading secret value from stdin");
        let stdin = io::stdin();
        let mut lines = stdin.lock().lines();
        lines
            .next()
            .ok_or_else(|| CliError::invalid_input("No input provided on stdin"))??
    } else if let Some(v) = &cmd.value {
        v.clone()
    } else {
        return Err(CliError::invalid_input(
            "Secret value required. Provide as argument or use --stdin",
        ));
    };

    // Build the URL
    let url = format!("{}/secrets/{}/{}", cmd.url, cmd.scope, cmd.name);
    tracing::debug!("POST {url}");

    // Encode value as base64
    let encoded_value = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());

    // Extract service/instance from name if scope requires it
    let (service, instance_id) = match cmd.scope.as_str() {
        "service" => {
            let service = cmd.name.split('/').next().map(String::from);
            (service, None)
        }
        "instance" => {
            let instance = cmd.name.split('/').next().map(String::from);
            (None, instance)
        }
        _ => (None, None),
    };

    let request_body = SetSecretRequest {
        value: encoded_value,
        service,
        instance_id,
    };

    // Make the request
    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", cmd.token))
        .json(&request_body)
        .send()
        .await
        .map_err(|e| CliError::general(format!("Failed to connect to keybox server: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.json::<ApiErrorResponse>().await.map_or_else(
            |_| format!("HTTP {status}"),
            |e| format!("{}: {}", e.error.code, e.error.message),
        );
        return Err(CliError::general(format!(
            "Failed to set secret: {error_body}"
        )));
    }

    let meta: SecretMetadataResponse = response
        .json()
        .await
        .map_err(|e| CliError::general(format!("Failed to parse response: {e}")))?;

    println!("Secret set: {}/{}", cmd.scope, cmd.name);
    println!("  Version: {}", meta.version);

    Ok(())
}

/// Run the `get` command.
pub async fn run_get(cmd: &GetArgs) -> Result<()> {
    // Build the URL
    let url = format!("{}/secrets/{}/{}", cmd.url, cmd.scope, cmd.name);
    tracing::debug!("GET {url}");

    // Make the request
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", cmd.token))
        .send()
        .await
        .map_err(|e| CliError::general(format!("Failed to connect to keybox server: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.json::<ApiErrorResponse>().await.map_or_else(
            |_| format!("HTTP {status}"),
            |e| format!("{}: {}", e.error.code, e.error.message),
        );
        return Err(CliError::general(format!(
            "Failed to get secret: {error_body}"
        )));
    }

    let resp: GetSecretResponse = response
        .json()
        .await
        .map_err(|e| CliError::general(format!("Failed to parse response: {e}")))?;

    // Decode the base64 value
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&resp.value)
        .map_err(|e| CliError::general(format!("Failed to decode secret value: {e}")))?;

    let value = String::from_utf8(decoded)
        .map_err(|e| CliError::general(format!("Secret value is not valid UTF-8: {e}")))?;

    // Output just the value (for scripting)
    println!("{value}");

    Ok(())
}

/// Run the `list` command.
pub async fn run_list(cmd: &ListArgs) -> Result<()> {
    // Build the URL based on scope and filters
    let url = match cmd.scope.as_str() {
        "service" if cmd.service.is_some() => {
            format!(
                "{}/secrets/service/{}",
                cmd.url,
                cmd.service.as_ref().unwrap()
            )
        }
        "instance" if cmd.instance.is_some() => {
            format!(
                "{}/secrets/instance/{}",
                cmd.url,
                cmd.instance.as_ref().unwrap()
            )
        }
        scope => format!("{}/secrets/{}", cmd.url, scope),
    };
    tracing::debug!("GET {url}");

    // Make the request
    let client = reqwest::Client::new();
    let response = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", cmd.token))
        .send()
        .await
        .map_err(|e| CliError::general(format!("Failed to connect to keybox server: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.json::<ApiErrorResponse>().await.map_or_else(
            |_| format!("HTTP {status}"),
            |e| format!("{}: {}", e.error.code, e.error.message),
        );
        return Err(CliError::general(format!(
            "Failed to list secrets: {error_body}"
        )));
    }

    let resp: ListSecretsResponse = response
        .json()
        .await
        .map_err(|e| CliError::general(format!("Failed to parse response: {e}")))?;

    if resp.secrets.is_empty() {
        println!("No secrets found in {} scope", cmd.scope);
        return Ok(());
    }

    println!("Secrets in {} scope:", cmd.scope);
    for secret in &resp.secrets {
        let qualifier = match (&secret.service, &secret.instance_id) {
            (Some(svc), _) => format!(" (service: {svc})"),
            (_, Some(inst)) => format!(" (instance: {inst})"),
            _ => String::new(),
        };
        println!("  {} (v{}){}", secret.name, secret.version, qualifier);
    }
    println!("\nTotal: {} secret(s)", resp.secrets.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_secret_request_serialization() {
        let req = SetSecretRequest {
            value: "c2VjcmV0".to_string(),
            service: None,
            instance_id: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""value":"c2VjcmV0""#));
        // service and instance_id should be omitted when None
        assert!(!json.contains("service"));
        assert!(!json.contains("instance_id"));
    }

    #[test]
    fn set_secret_request_with_service() {
        let req = SetSecretRequest {
            value: "c2VjcmV0".to_string(),
            service: Some("tokenization".to_string()),
            instance_id: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""service":"tokenization""#));
    }

    #[test]
    fn api_error_response_deserialization() {
        let json = r#"{"error":{"code":"SECRET_NOT_FOUND","message":"Secret not found"}}"#;
        let resp: ApiErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error.code, "SECRET_NOT_FOUND");
        assert_eq!(resp.error.message, "Secret not found");
    }

    #[test]
    fn get_secret_response_deserialization() {
        let json = r#"{
            "name": "ai/anthropic",
            "scope": "global",
            "version": 1,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z",
            "value": "c2VjcmV0"
        }"#;
        let resp: GetSecretResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.metadata.name, "ai/anthropic");
        assert_eq!(resp.metadata.scope, "global");
        assert_eq!(resp.value, "c2VjcmV0");
    }

    #[test]
    fn list_secrets_response_deserialization() {
        let json = r#"{
            "secrets": [
                {"name": "ai/anthropic", "scope": "global", "version": 1},
                {"name": "ai/openai", "scope": "global", "version": 2}
            ]
        }"#;
        let resp: ListSecretsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.secrets.len(), 2);
        assert_eq!(resp.secrets[0].name, "ai/anthropic");
        assert_eq!(resp.secrets[1].name, "ai/openai");
    }
}
