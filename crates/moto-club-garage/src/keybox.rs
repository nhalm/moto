//! Keybox client for issuing garage SVIDs.
//!
//! Per moto-club.md spec v1.3 step 8, moto-club calls keybox's
//! `POST /auth/issue-garage-svid` endpoint to issue an SVID for a garage.
//!
//! This module provides a minimal HTTP client for this single operation.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, instrument};

/// Errors from keybox client operations.
#[derive(Debug, Error)]
pub enum KeyboxError {
    /// HTTP request failed.
    #[error("keybox request failed: {0}")]
    Request(#[from] reqwest::Error),

    /// Keybox returned an error response.
    #[error("keybox error: {code} - {message}")]
    Api {
        /// Error code.
        code: String,
        /// Error message.
        message: String,
    },

    /// Keybox URL not configured.
    #[error("keybox URL not configured")]
    NotConfigured,
}

/// Response from `POST /auth/issue-garage-svid`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueGarageSvidResponse {
    /// The signed SVID JWT (1 hour TTL).
    pub token: String,
    /// Token expiration time (Unix timestamp).
    pub expires_at: i64,
    /// The SPIFFE ID for this garage.
    pub spiffe_id: String,
}

/// Request body for `POST /auth/issue-garage-svid`.
#[derive(Debug, Clone, Serialize)]
struct IssueGarageSvidRequest {
    /// The garage UUID.
    garage_id: String,
    /// The garage owner identifier.
    owner: String,
}

/// Error response from keybox API.
#[derive(Debug, Clone, Deserialize)]
struct ApiErrorResponse {
    error: ApiErrorDetail,
}

#[derive(Debug, Clone, Deserialize)]
struct ApiErrorDetail {
    code: String,
    message: String,
}

/// Keybox client for garage SVID operations.
#[derive(Clone)]
pub struct KeyboxClient {
    client: Client,
    base_url: String,
    service_token: String,
}

impl KeyboxClient {
    /// Creates a new keybox client.
    ///
    /// # Arguments
    ///
    /// * `base_url` - The keybox server URL (e.g., `http://keybox:8080`)
    /// * `service_token` - The service token for authentication
    #[must_use]
    pub fn new(base_url: impl Into<String>, service_token: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
            service_token: service_token.into(),
        }
    }

    /// Issues an SVID for a garage.
    ///
    /// Per keybox.md spec v0.3, this endpoint allows moto-club to request
    /// an SVID on behalf of a garage. Garage SVIDs have a 1-hour TTL.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or keybox returns an error.
    #[instrument(skip(self), fields(garage_id = %garage_id, owner = %owner))]
    pub async fn issue_garage_svid(
        &self,
        garage_id: &str,
        owner: &str,
    ) -> Result<IssueGarageSvidResponse, KeyboxError> {
        let url = format!("{}/auth/issue-garage-svid", self.base_url);

        debug!(url = %url, "issuing garage SVID");

        let request = IssueGarageSvidRequest {
            garage_id: garage_id.to_string(),
            owner: owner.to_string(),
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.service_token))
            .json(&request)
            .send()
            .await?;

        if response.status().is_success() {
            let svid_response: IssueGarageSvidResponse = response.json().await?;
            debug!(
                spiffe_id = %svid_response.spiffe_id,
                expires_at = svid_response.expires_at,
                "garage SVID issued successfully"
            );
            Ok(svid_response)
        } else {
            // Try to parse error response
            let status = response.status();
            let error = match response.json::<ApiErrorResponse>().await {
                Ok(err) => KeyboxError::Api {
                    code: err.error.code,
                    message: err.error.message,
                },
                Err(_) => KeyboxError::Api {
                    code: "UNKNOWN".to_string(),
                    message: format!("HTTP {status}"),
                },
            };
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_garage_svid_response_deserialize() {
        let json = r#"{"token":"eyJ...","expires_at":1700003600,"spiffe_id":"spiffe://moto.local/garage/abc123"}"#;
        let response: IssueGarageSvidResponse = serde_json::from_str(json).unwrap();

        assert_eq!(response.token, "eyJ...");
        assert_eq!(response.expires_at, 1_700_003_600);
        assert_eq!(response.spiffe_id, "spiffe://moto.local/garage/abc123");
    }

    #[test]
    fn issue_garage_svid_request_serialize() {
        let request = IssueGarageSvidRequest {
            garage_id: "abc123".to_string(),
            owner: "nick".to_string(),
        };
        let json = serde_json::to_string(&request).unwrap();

        assert!(json.contains(r#""garage_id":"abc123""#));
        assert!(json.contains(r#""owner":"nick""#));
    }

    #[test]
    fn keybox_error_display() {
        let err = KeyboxError::Api {
            code: "INVALID_SERVICE_TOKEN".to_string(),
            message: "Invalid service token".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "keybox error: INVALID_SERVICE_TOKEN - Invalid service token"
        );

        let err = KeyboxError::NotConfigured;
        assert_eq!(err.to_string(), "keybox URL not configured");
    }
}
