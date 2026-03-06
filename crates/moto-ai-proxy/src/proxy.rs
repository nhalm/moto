//! Core proxy logic — forwards requests to upstream AI providers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use reqwest::Client;
use secrecy::ExposeSecret;
use uuid::Uuid;

use crate::auth::{self, AuthError, GarageValidator};
use crate::keys::KeyStore;
use crate::provider::Provider;

/// Maximum request body size (10 MB).
const MAX_REQUEST_BODY_SIZE: usize = 10 * 1024 * 1024;

/// Time to first response byte from upstream (30s).
const FIRST_BYTE_TIMEOUT: Duration = Duration::from_secs(30);

/// Shared state for proxy handlers.
pub struct ProxyState<K: KeyStore + 'static, G: GarageValidator + 'static> {
    /// HTTP client for upstream requests.
    pub client: Client,
    /// Key store for fetching provider API keys.
    pub key_store: Arc<K>,
    /// Garage identity validator.
    pub garage_validator: Arc<G>,
}

impl<K: KeyStore + 'static, G: GarageValidator + 'static> Clone for ProxyState<K, G> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            key_store: Arc::clone(&self.key_store),
            garage_validator: Arc::clone(&self.garage_validator),
        }
    }
}

/// Validates the garage identity from request headers.
///
/// Extracts the auth token, parses the garage ID, and validates via the garage validator.
async fn validate_garage_auth<G: GarageValidator>(
    headers: &HeaderMap,
    validator: &G,
) -> Result<String, Response> {
    let token = auth::extract_token(headers).ok_or_else(|| {
        let err = AuthError::MissingToken;
        error_response(err.status_code(), &err.message(), err.error_type())
    })?;

    let garage_id = auth::extract_garage_id(&token).ok_or_else(|| {
        let err = AuthError::InvalidToken;
        error_response(err.status_code(), &err.message(), err.error_type())
    })?;

    validator
        .validate_garage(&garage_id)
        .await
        .map_err(|err| error_response(err.status_code(), &err.message(), err.error_type()))?;

    Ok(garage_id)
}

/// Result of forwarding a request to an upstream provider.
pub struct ForwardResult {
    /// The HTTP response to return to the client.
    pub response: Response,
    /// The upstream HTTP status code, if a response was received.
    pub upstream_status: Option<u16>,
}

/// Forwards a request to the given provider's upstream, rewriting the path
/// and injecting the real API key from keybox.
#[allow(clippy::too_many_arguments)]
pub async fn forward_to_provider<K: KeyStore>(
    provider: Provider,
    method: Method,
    remaining_path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Body,
    client: &Client,
    key_store: &K,
) -> ForwardResult {
    // Fetch the API key for this provider.
    let Some(api_key) = key_store.get_key(provider).await else {
        return ForwardResult {
            response: error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                &format!("provider not configured: {provider}"),
                "server_error",
            ),
            upstream_status: None,
        };
    };

    let base = provider.upstream_base();
    let mut upstream_url = format!("{base}{remaining_path}");
    if let Some(q) = query {
        upstream_url.push('?');
        upstream_url.push_str(q);
    }

    // Build the upstream request, forwarding Content-Type and Accept headers.
    let mut req = client.request(reqwest_method(&method), &upstream_url);

    // Inject the real API key using the provider-specific auth header.
    req = req.header(
        provider.auth_header(),
        provider.auth_value(api_key.expose_secret()),
    );

    if let Some(ct) = headers.get("content-type")
        && let Ok(v) = ct.to_str()
    {
        req = req.header("content-type", v);
    }
    if let Some(accept) = headers.get("accept")
        && let Ok(v) = accept.to_str()
    {
        req = req.header("accept", v);
    }

    // Forward anthropic-version header for Anthropic passthrough.
    if provider == Provider::Anthropic
        && let Some(av) = headers.get("anthropic-version")
        && let Ok(v) = av.to_str()
    {
        req = req.header("anthropic-version", v);
    }

    // Stream the body through via reqwest's body wrapper.
    req = req.body(reqwest::Body::wrap_stream(body.into_data_stream()));

    // First-byte timeout: time to receive response headers from upstream.
    let upstream_resp = match tokio::time::timeout(FIRST_BYTE_TIMEOUT, req.send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            tracing::error!(provider = %provider, error = %e, "upstream request failed");
            return ForwardResult {
                response: error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("upstream error: {provider}"),
                    "server_error",
                ),
                upstream_status: None,
            };
        }
        Err(_) => {
            tracing::error!(provider = %provider, "upstream first byte timeout ({}s)", FIRST_BYTE_TIMEOUT.as_secs());
            return ForwardResult {
                response: error_response(
                    StatusCode::GATEWAY_TIMEOUT,
                    &format!("upstream timeout: {provider}"),
                    "server_error",
                ),
                upstream_status: None,
            };
        }
    };

    // Convert upstream response back to axum Response, streaming the body.
    let upstream_status_code = upstream_resp.status().as_u16();
    let status = StatusCode::from_u16(upstream_status_code).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut response_headers = HeaderMap::new();

    // Forward Content-Type from upstream (text/event-stream for SSE).
    if let Some(ct) = upstream_resp.headers().get("content-type") {
        response_headers.insert("content-type", ct.clone());
    }
    // Forward Transfer-Encoding for streaming (chunked).
    if let Some(te) = upstream_resp.headers().get("transfer-encoding") {
        response_headers.insert("transfer-encoding", te.clone());
    }
    // Forward Cache-Control from upstream (SSE uses no-cache).
    if let Some(cc) = upstream_resp.headers().get("cache-control") {
        response_headers.insert("cache-control", cc.clone());
    }

    let body = Body::from_stream(upstream_resp.bytes_stream());

    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    *resp.headers_mut() = response_headers;

    ForwardResult {
        response: resp,
        upstream_status: Some(upstream_status_code),
    }
}

/// Shared passthrough handler — validates auth, forwards to provider, emits canonical log.
async fn handle_passthrough<K: KeyStore, G: GarageValidator>(
    provider: Provider,
    state: &ProxyState<K, G>,
    method: Method,
    uri: &Uri,
    headers: &HeaderMap,
    remaining: &str,
    body: Body,
) -> Response {
    let request_id = Uuid::now_v7();
    let start = Instant::now();
    let method_str = method.to_string();
    let path = uri.path().to_string();

    let garage_id = match validate_garage_auth(headers, state.garage_validator.as_ref()).await {
        Ok(id) => id,
        Err(resp) => {
            let status = resp.status().as_u16();
            let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            tracing::info!(
                request_id = %request_id,
                garage_id = "",
                provider = %provider,
                mode = "passthrough",
                method = %method_str,
                path = %path,
                status = status,
                duration_ms = duration_ms,
                "request completed"
            );
            return resp;
        }
    };

    let result = forward_to_provider(
        provider,
        method,
        remaining,
        uri.query(),
        headers,
        body,
        &state.client,
        state.key_store.as_ref(),
    )
    .await;

    let status = result.response.status().as_u16();
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        request_id = %request_id,
        garage_id = %garage_id,
        provider = %provider,
        mode = "passthrough",
        method = %method_str,
        path = %path,
        status = status,
        upstream_status = result.upstream_status,
        duration_ms = duration_ms,
        "request completed"
    );

    result.response
}

/// Handler for `/passthrough/anthropic/*path`.
pub async fn passthrough_anthropic<K: KeyStore, G: GarageValidator>(
    State(state): State<ProxyState<K, G>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(remaining): Path<String>,
    body: Body,
) -> Response {
    handle_passthrough(
        Provider::Anthropic,
        &state,
        method,
        &uri,
        &headers,
        &remaining,
        body,
    )
    .await
}

/// Handler for `/passthrough/openai/*path`.
pub async fn passthrough_openai<K: KeyStore, G: GarageValidator>(
    State(state): State<ProxyState<K, G>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(remaining): Path<String>,
    body: Body,
) -> Response {
    handle_passthrough(
        Provider::OpenAi,
        &state,
        method,
        &uri,
        &headers,
        &remaining,
        body,
    )
    .await
}

/// Handler for `/passthrough/gemini/*path`.
pub async fn passthrough_gemini<K: KeyStore, G: GarageValidator>(
    State(state): State<ProxyState<K, G>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(remaining): Path<String>,
    body: Body,
) -> Response {
    handle_passthrough(
        Provider::Gemini,
        &state,
        method,
        &uri,
        &headers,
        &remaining,
        body,
    )
    .await
}

/// Handler for `POST /v1/chat/completions` — unified endpoint.
///
/// Reads the request body, extracts the `model` field, and routes to the
/// correct provider. For `OpenAI` and Gemini, the request is forwarded directly
/// (both support OpenAI-compatible format). Anthropic requires translation
/// (handled by the translation layer when implemented).
pub async fn chat_completions<K: KeyStore, G: GarageValidator>(
    State(state): State<ProxyState<K, G>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = Uuid::now_v7();
    let start = Instant::now();
    let path = uri.path().to_string();

    let garage_id = match validate_garage_auth(&headers, state.garage_validator.as_ref()).await {
        Ok(id) => id,
        Err(resp) => {
            let status = resp.status().as_u16();
            let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            tracing::info!(
                request_id = %request_id,
                garage_id = "",
                mode = "unified",
                method = "POST",
                path = %path,
                status = status,
                duration_ms = duration_ms,
                "request completed"
            );
            return resp;
        }
    };

    // Parse the model field from the request body.
    let model = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("model").and_then(|m| m.as_str()).map(String::from));

    let Some(model) = model else {
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        tracing::info!(
            request_id = %request_id,
            garage_id = %garage_id,
            mode = "unified",
            method = "POST",
            path = %path,
            status = 400u16,
            duration_ms = duration_ms,
            "request completed"
        );
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing or invalid 'model' field in request body",
            "invalid_request_error",
        );
    };

    let Some(provider) = Provider::from_model(&model) else {
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        tracing::info!(
            request_id = %request_id,
            garage_id = %garage_id,
            model = %model,
            mode = "unified",
            method = "POST",
            path = %path,
            status = 400u16,
            duration_ms = duration_ms,
            "request completed"
        );
        return error_response(
            StatusCode::BAD_REQUEST,
            "unknown model prefix, cannot determine provider",
            "invalid_request_error",
        );
    };

    let upstream_path = provider.unified_chat_path();

    // Forward the buffered body to the upstream provider.
    let result = forward_to_provider(
        provider,
        Method::POST,
        upstream_path,
        uri.query(),
        &headers,
        Body::from(body),
        &state.client,
        state.key_store.as_ref(),
    )
    .await;

    let status = result.response.status().as_u16();
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        request_id = %request_id,
        garage_id = %garage_id,
        provider = %provider,
        model = %model,
        mode = "unified",
        method = "POST",
        path = %path,
        status = status,
        upstream_status = result.upstream_status,
        duration_ms = duration_ms,
        "request completed"
    );

    result.response
}

/// Builds the proxy router with passthrough routes and garage auth.
pub fn proxy_router<K: KeyStore + 'static, G: GarageValidator + 'static>(
    client: Client,
    key_store: K,
    garage_validator: G,
) -> axum::Router {
    let state = ProxyState {
        client,
        key_store: Arc::new(key_store),
        garage_validator: Arc::new(garage_validator),
    };
    axum::Router::new()
        .route(
            "/passthrough/anthropic/{*path}",
            axum::routing::any(passthrough_anthropic::<K, G>),
        )
        .route(
            "/passthrough/openai/{*path}",
            axum::routing::any(passthrough_openai::<K, G>),
        )
        .route(
            "/passthrough/gemini/{*path}",
            axum::routing::any(passthrough_gemini::<K, G>),
        )
        .route(
            "/v1/chat/completions",
            axum::routing::post(chat_completions::<K, G>),
        )
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_SIZE))
        .with_state(state)
}

/// Returns a JSON error response in `OpenAI` error format.
#[must_use]
pub fn error_response(status: StatusCode, message: &str, error_type: &str) -> Response {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "type": error_type,
        }
    });
    (
        status,
        [(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        body.to_string(),
    )
        .into_response()
}

/// Maps axum's `http::Method` to `reqwest::Method`.
const fn reqwest_method(method: &Method) -> reqwest::Method {
    match *method {
        Method::POST => reqwest::Method::POST,
        Method::PUT => reqwest::Method::PUT,
        Method::DELETE => reqwest::Method::DELETE,
        Method::PATCH => reqwest::Method::PATCH,
        Method::HEAD => reqwest::Method::HEAD,
        Method::OPTIONS => reqwest::Method::OPTIONS,
        // GET and any other method
        _ => reqwest::Method::GET,
    }
}
