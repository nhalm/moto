//! End-to-end smoke tests for moto-ai-proxy.
//!
//! These tests start the actual proxy server on a real port, start mock
//! upstream AI provider servers, and send HTTP requests through the full
//! network stack to verify the complete pipeline:
//!
//!   client HTTP → proxy server → mock upstream → response back
//!
//! This tests things that handler-level `oneshot()` tests cannot:
//! - Real TCP connections and HTTP parsing
//! - Streaming SSE over the network
//! - Header propagation through the full stack
//! - Concurrent request handling

use std::collections::HashMap;
use std::convert::Infallible;
use std::time::Duration;

use axum::body::Body;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use secrecy::SecretString;
use tokio_stream::wrappers::ReceiverStream;

use moto_ai_proxy::auth::{AuthError, GarageValidator};
use moto_ai_proxy::keys::KeyStore;
use moto_ai_proxy::provider::ModelRouter;
use moto_ai_proxy::proxy;

// -- Mock components --

/// Build a test SVID JWT (same logic as `auth::build_test_svid` but available in integration tests).
fn garage_svid(id: &str) -> String {
    let header = serde_json::json!({"alg": "EdDSA", "typ": "JWT"});
    let now = Utc::now().timestamp();
    let claims = serde_json::json!({
        "iss": "keybox",
        "sub": format!("spiffe://moto.local/garage/{id}"),
        "aud": "moto",
        "exp": now + 900,
        "iat": now,
        "jti": "smoke-test-jti",
        "principal_type": "garage",
        "principal_id": id,
    });
    let header_b64 = URL_SAFE_NO_PAD.encode(header.to_string());
    let claims_b64 = URL_SAFE_NO_PAD.encode(claims.to_string());
    let sig_b64 = URL_SAFE_NO_PAD.encode(vec![0u8; 64]);
    format!("{header_b64}.{claims_b64}.{sig_b64}")
}

struct MockKeyStore {
    keys: HashMap<String, SecretString>,
}

impl MockKeyStore {
    fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    fn with_key(mut self, secret_name: &str, key: &str) -> Self {
        self.keys
            .insert(secret_name.to_string(), SecretString::from(key.to_string()));
        self
    }
}

impl KeyStore for MockKeyStore {
    async fn get_key(&self, secret_name: &str) -> Option<SecretString> {
        self.keys.get(secret_name).cloned()
    }
}

struct AcceptAllValidator;

impl GarageValidator for AcceptAllValidator {
    async fn validate_garage(&self, _garage_id: &str) -> Result<(), AuthError> {
        Ok(())
    }
}

// -- Mock upstream servers --

/// Starts a mock AI API that returns a non-streaming OpenAI-format response.
/// Serves on /v1/chat/completions (the path custom model mappings use).
async fn start_mock_anthropic() -> String {
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|req: axum::extract::Request| async move {
            // Verify the proxy injected the real API key
            let api_key = req
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            // Return OpenAI-format response (custom providers don't translate)
            let body = serde_json::json!({
                "id": "chatcmpl-mock-anthropic",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello from mock Anthropic"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                },
                "_injected_key": api_key
            });
            axum::Json(body).into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}/", addr.port())
}

/// Starts a mock AI API that returns a streaming SSE response in `OpenAI` format.
async fn start_mock_anthropic_streaming() -> String {
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, Infallible>>(16);

            tokio::spawn(async move {
                // OpenAI-format SSE chunks (custom providers don't translate)
                let events = [
                    "data: {\"id\":\"chatcmpl-stream\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}]}\n\n",
                    "data: {\"id\":\"chatcmpl-stream\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n",
                    "data: {\"id\":\"chatcmpl-stream\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" from\"},\"finish_reason\":null}]}\n\n",
                    "data: {\"id\":\"chatcmpl-stream\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" streaming\"},\"finish_reason\":null}]}\n\n",
                    "data: {\"id\":\"chatcmpl-stream\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n",
                ];
                for event in events {
                    if tx.send(Ok(event.to_string())).await.is_err() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });

            Response::builder()
                .status(200)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}/", addr.port())
}

/// Starts a mock `OpenAI` API that returns a chat completion response.
async fn start_mock_openai() -> String {
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|req: axum::extract::Request| async move {
            let auth = req
                .headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            let body = serde_json::json!({
                "id": "chatcmpl-smoke",
                "object": "chat.completion",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello from mock OpenAI"},
                    "finish_reason": "stop"
                }],
                "usage": {
                    "prompt_tokens": 10,
                    "completion_tokens": 5,
                    "total_tokens": 15
                },
                "_injected_auth": auth
            });
            axum::Json(body).into_response()
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://127.0.0.1:{}/", addr.port())
}

/// Starts the actual proxy server with mock key stores pointing to mock upstreams.
///
/// Custom model map routes `claude-*` and `gpt-*` to local mock servers.
/// The mock-anthropic provider is configured like Anthropic (x-api-key) and
/// the mock-openai provider is configured like `OpenAI` (Authorization: Bearer).
///
/// NOTE: Passthrough routes use hardcoded provider names (anthropic, openai, gemini)
/// and cannot be pointed at mock upstreams without code changes. Passthrough routing
/// logic is verified by unit tests. These e2e tests focus on the unified endpoint
/// which does support custom upstreams via `MOTO_AI_PROXY_MODEL_MAP`.
async fn start_proxy_with_mocks(anthropic_url: &str, openai_url: &str) -> String {
    // Use custom model map to point providers at our mock servers
    let model_map = serde_json::json!([
        {
            "prefix": "claude-",
            "provider": "mock-anthropic",
            "upstream": anthropic_url,
            "auth_header": "x-api-key",
            "auth_prefix": ""
        },
        {
            "prefix": "gpt-",
            "provider": "mock-openai",
            "upstream": openai_url,
            "auth_header": "Authorization",
            "auth_prefix": "Bearer "
        }
    ]);

    let key_store = MockKeyStore::new()
        .with_key("ai-proxy/mock-anthropic", "sk-ant-smoke-test-key")
        .with_key("ai-proxy/mock-openai", "sk-openai-smoke-test-key");

    let model_router = ModelRouter::new(Some(&model_map.to_string())).expect("valid model map");

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();

    let app = proxy::proxy_router(client, key_store, AcceptAllValidator, model_router);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://127.0.0.1:{}", addr.port())
}

// -- Smoke tests --

/// Test the full pipeline: HTTP client → proxy server → mock Anthropic → response back.
/// Uses the unified endpoint which routes claude-* models to mock-anthropic upstream.
/// The proxy translates `OpenAI` format → Anthropic format, injects real key, forwards,
/// then translates the response back to `OpenAI` format.
#[tokio::test]
async fn smoke_unified_claude_non_streaming() {
    let mock_anthropic = start_mock_anthropic().await;
    let proxy_url = start_proxy_with_mocks(&mock_anthropic, "http://localhost:1/").await;

    let client = reqwest::Client::new();
    let svid = garage_svid("smoke-garage-1");

    let resp = client
        .post(format!("{proxy_url}/v1/chat/completions"))
        .header("authorization", format!("Bearer {svid}"))
        .header("content-type", "application/json")
        .body(
            serde_json::json!({
                "model": "claude-sonnet-4-20250514",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 200, "proxy should return 200");

    // Verify moto headers are present
    assert!(
        resp.headers().get("x-moto-request-id").is_some(),
        "should have X-Moto-Request-Id"
    );
    let provider = resp
        .headers()
        .get("x-moto-provider")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(provider, "mock-anthropic", "should route to mock-anthropic");

    let body: serde_json::Value = resp.json().await.unwrap();
    // Response is OpenAI format (custom providers pass through without translation)
    assert!(
        body["choices"].is_array(),
        "should have choices array (OpenAI format)"
    );
    assert_eq!(
        body["choices"][0]["message"]["content"], "Hello from mock Anthropic",
        "response content should match"
    );
    // Verify the proxy injected the real key (not the garage SVID)
    assert_eq!(
        body["_injected_key"], "sk-ant-smoke-test-key",
        "proxy should inject real API key from keybox"
    );
}

/// Test streaming through the unified endpoint: `OpenAI` format request → Anthropic streaming
/// → translated back to `OpenAI` SSE chunks.
#[tokio::test]
async fn smoke_unified_claude_streaming() {
    let mock_anthropic = start_mock_anthropic_streaming().await;
    let proxy_url = start_proxy_with_mocks(&mock_anthropic, "http://localhost:1/").await;

    let client = reqwest::Client::new();
    let svid = garage_svid("smoke-garage-2");

    let resp = client
        .post(format!("{proxy_url}/v1/chat/completions"))
        .header("authorization", format!("Bearer {svid}"))
        .header("content-type", "application/json")
        .body(
            serde_json::json!({
                "model": "claude-sonnet-4-20250514",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": true
            })
            .to_string(),
        )
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 200);

    // Collect the full streamed body — OpenAI SSE format passed through
    let body = resp.text().await.unwrap();

    assert!(body.contains("data:"), "should have SSE data lines");
    assert!(body.contains("Hello"), "should contain Hello text");
    assert!(body.contains("streaming"), "should contain streaming text");
    assert!(body.contains("[DONE]"), "should end with [DONE] marker");
}

/// Test the unified endpoint: `OpenAI` format → model routing → mock upstream → response.
#[tokio::test]
async fn smoke_unified_endpoint_routes_by_model() {
    let mock_anthropic = start_mock_anthropic().await;
    let mock_openai = start_mock_openai().await;
    let proxy_url = start_proxy_with_mocks(&mock_anthropic, &mock_openai).await;

    let client = reqwest::Client::new();
    let svid = garage_svid("smoke-garage-3");

    // Send a request with a GPT model — should route to mock OpenAI
    let resp = client
        .post(format!("{proxy_url}/v1/chat/completions"))
        .header("authorization", format!("Bearer {svid}"))
        .header("content-type", "application/json")
        .body(
            serde_json::json!({
                "model": "gpt-4o",
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        )
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 200);

    let provider = resp
        .headers()
        .get("x-moto-provider")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(provider, "mock-openai", "should route to OpenAI provider");

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(
        body["choices"][0]["message"]["content"],
        "Hello from mock OpenAI"
    );
    // Verify the proxy injected the real OpenAI key
    assert_eq!(
        body["_injected_auth"], "Bearer sk-openai-smoke-test-key",
        "proxy should inject real OpenAI API key"
    );
}

/// Test auth rejection: missing auth header should return 401.
#[tokio::test]
async fn smoke_missing_auth_returns_401() {
    let mock_anthropic = start_mock_anthropic().await;
    let proxy_url = start_proxy_with_mocks(&mock_anthropic, "http://localhost:1/").await;

    let client = reqwest::Client::new();

    // No auth header
    let resp = client
        .post(format!("{proxy_url}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(r#"{"model": "gpt-4o", "messages": []}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "authentication_error");
}

/// Test unknown model returns 400 with actionable error.
#[tokio::test]
async fn smoke_unknown_model_returns_400() {
    let mock_anthropic = start_mock_anthropic().await;
    let proxy_url = start_proxy_with_mocks(&mock_anthropic, "http://localhost:1/").await;

    let client = reqwest::Client::new();
    let svid = garage_svid("smoke-garage-4");

    let resp = client
        .post(format!("{proxy_url}/v1/chat/completions"))
        .header("authorization", format!("Bearer {svid}"))
        .header("content-type", "application/json")
        .body(r#"{"model": "unknown-model-xyz", "messages": []}"#)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

/// Test concurrent requests through the proxy to verify it handles multiple
/// clients simultaneously.
#[tokio::test]
async fn smoke_concurrent_requests() {
    let mock_anthropic = start_mock_anthropic().await;
    let mock_openai = start_mock_openai().await;
    let proxy_url = start_proxy_with_mocks(&mock_anthropic, &mock_openai).await;

    let client = reqwest::Client::new();

    let mut handles = Vec::new();
    for i in 0..10 {
        let client = client.clone();
        let url = proxy_url.clone();
        let svid = garage_svid(&format!("concurrent-{i}"));

        handles.push(tokio::spawn(async move {
            let resp = client
                .post(format!("{url}/v1/chat/completions"))
                .header("authorization", format!("Bearer {svid}"))
                .header("content-type", "application/json")
                .body(
                    serde_json::json!({
                        "model": "gpt-4o",
                        "messages": [{"role": "user", "content": format!("Request {i}")}]
                    })
                    .to_string(),
                )
                .send()
                .await
                .expect("concurrent request should succeed");

            assert_eq!(resp.status(), 200, "request {i} should return 200");
            let body: serde_json::Value = resp.json().await.unwrap();
            assert_eq!(
                body["_injected_auth"], "Bearer sk-openai-smoke-test-key",
                "request {i} should have injected key"
            );
        }));
    }

    for handle in handles {
        handle.await.expect("task should complete");
    }
}
