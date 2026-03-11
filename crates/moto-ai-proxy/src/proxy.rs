//! Core proxy logic — forwards requests to upstream AI providers.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use reqwest::Client;
use secrecy::ExposeSecret;
use serde_json::Value;
use uuid::Uuid;

use crate::audit;
use crate::auth::{self, AuthError, GarageValidator};
use crate::keys::KeyStore;
use crate::provider::{ModelRouter, Provider, ProviderInfo};
use crate::translate::anthropic as anthropic_translate;
use moto_keybox::svid::SvidValidator;

/// Known models to return for Anthropic (no public /v1/models endpoint).
const ANTHROPIC_KNOWN_MODELS: &[&str] = &[
    "claude-opus-4-20250514",
    "claude-sonnet-4-20250514",
    "claude-haiku-4-20250414",
];

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
    /// SVID validator for verifying garage identity signatures.
    pub svid_validator: Arc<SvidValidator>,
    /// Garage identity validator.
    pub garage_validator: Arc<G>,
    /// Model router for resolving model names to providers.
    pub model_router: ModelRouter,
}

impl<K: KeyStore + 'static, G: GarageValidator + 'static> Clone for ProxyState<K, G> {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            key_store: Arc::clone(&self.key_store),
            svid_validator: Arc::clone(&self.svid_validator),
            garage_validator: Arc::clone(&self.garage_validator),
            model_router: self.model_router.clone(),
        }
    }
}

/// Validates the garage identity from request headers.
///
/// Extracts the auth token, verifies the SVID Ed25519 signature, extracts the
/// garage ID, and validates via the garage validator.
async fn validate_garage_auth<G: GarageValidator>(
    headers: &HeaderMap,
    svid_validator: &SvidValidator,
    garage_validator: &G,
) -> Result<String, Response> {
    let token = auth::extract_token(headers).ok_or_else(|| {
        let err = AuthError::MissingToken;
        error_response(err.status_code(), &err.message(), err.error_type())
    })?;

    let garage_id = auth::extract_garage_id(svid_validator, &token)
        .map_err(|err| error_response(err.status_code(), &err.message(), err.error_type()))?;

    garage_validator
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
    /// The upstream response headers (for audit logging).
    pub upstream_headers: Option<HeaderMap>,
}

/// Forwards a request to the given provider's upstream, rewriting the path
/// and injecting the real API key from keybox.
#[allow(clippy::too_many_arguments)]
pub async fn forward_to_provider<K: KeyStore>(
    info: &ProviderInfo,
    method: Method,
    remaining_path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Body,
    client: &Client,
    key_store: &K,
) -> ForwardResult {
    // Fetch the API key for this provider.
    let Some(api_key) = key_store.get_key(&info.secret_name).await else {
        return ForwardResult {
            response: error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                &format!("provider not configured: {}", info.name),
                "server_error",
            ),
            upstream_status: None,
            upstream_headers: None,
        };
    };

    let mut upstream_url = format!("{}{remaining_path}", info.upstream_base);
    if let Some(q) = query {
        upstream_url.push('?');
        upstream_url.push_str(q);
    }

    // Build the upstream request, forwarding Content-Type and Accept headers.
    let mut req = client.request(reqwest_method(&method), &upstream_url);

    // Inject the real API key using the provider-specific auth header.
    req = req.header(&*info.auth_header, info.auth_value(api_key.expose_secret()));

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

    // Forward anthropic-version header for Anthropic requests.
    // Use the client-provided value if present, otherwise default to a known version.
    if info.is_anthropic {
        if let Some(av) = headers.get("anthropic-version")
            && let Ok(v) = av.to_str()
        {
            req = req.header("anthropic-version", v);
        } else {
            req = req.header("anthropic-version", "2023-06-01");
        }
    }

    // Stream the body through via reqwest's body wrapper.
    req = req.body(reqwest::Body::wrap_stream(body.into_data_stream()));

    // First-byte timeout: time to receive response headers from upstream.
    let upstream_resp = match tokio::time::timeout(FIRST_BYTE_TIMEOUT, req.send()).await {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            tracing::error!(provider = %info.name, error = %e, "upstream request failed");
            return ForwardResult {
                response: error_response(
                    StatusCode::BAD_GATEWAY,
                    &format!("upstream error: {}", info.name),
                    "server_error",
                ),
                upstream_status: None,
                upstream_headers: None,
            };
        }
        Err(_) => {
            tracing::error!(provider = %info.name, "upstream first byte timeout ({}s)", FIRST_BYTE_TIMEOUT.as_secs());
            return ForwardResult {
                response: error_response(
                    StatusCode::GATEWAY_TIMEOUT,
                    &format!("upstream timeout: {}", info.name),
                    "server_error",
                ),
                upstream_status: None,
                upstream_headers: None,
            };
        }
    };

    // Convert upstream response back to axum Response, streaming the body.
    let upstream_status_code = upstream_resp.status().as_u16();
    let status = StatusCode::from_u16(upstream_status_code).unwrap_or(StatusCode::BAD_GATEWAY);

    // Capture upstream headers for audit logging (before consuming the response).
    let upstream_headers = upstream_resp.headers().clone();

    // Sanitize non-success responses: buffer, extract error message, scrub API keys,
    // and wrap in OpenAI error format. Raw upstream error bodies are never forwarded.
    if !upstream_resp.status().is_success() {
        let sanitized = sanitize_upstream_error(&info.name, upstream_resp, status).await;
        return ForwardResult {
            response: sanitized,
            upstream_status: Some(upstream_status_code),
            upstream_headers: Some(upstream_headers),
        };
    }

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
        upstream_headers: Some(upstream_headers),
    }
}

/// Injects `X-Moto-Request-Id` and `X-Moto-Provider` response headers.
fn inject_moto_headers(response: &mut Response, request_id: &Uuid, provider: Option<&str>) {
    if let Ok(val) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert("x-moto-request-id", val);
    }
    if let Some(name) = provider
        && let Ok(val) = HeaderValue::from_str(name)
    {
        response.headers_mut().insert("x-moto-provider", val);
    }
}

/// Shared passthrough handler — validates auth, forwards to provider, emits canonical log.
#[allow(clippy::too_many_lines)]
async fn handle_passthrough<K: KeyStore, G: GarageValidator>(
    provider: Provider,
    state: &ProxyState<K, G>,
    method: Method,
    uri: &Uri,
    headers: &HeaderMap,
    remaining: &str,
    body: Body,
) -> Response {
    let info = provider.info();
    let request_id = Uuid::now_v7();
    let start = Instant::now();
    let method_str = method.to_string();
    let path = uri.path().to_string();

    // Check passthrough path allowlist before auth (fail fast on disallowed paths).
    if !provider.is_path_allowed(remaining) {
        let mut resp = error_response(StatusCode::FORBIDDEN, "path not allowed", "forbidden");
        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        tracing::info!(
            request_id = %request_id,
            garage_id = "",
            provider = %info.name,
            mode = "passthrough",
            method = %method_str,
            path = %path,
            status = 403u16,
            duration_ms = duration_ms,
            "request completed"
        );
        audit::log_ai_request_denied(
            &request_id,
            "",
            "path not allowed",
            Some(&info.name),
            "passthrough",
            &start,
            headers,
        );
        inject_moto_headers(&mut resp, &request_id, Some(&info.name));
        return resp;
    }

    let garage_id = match validate_garage_auth(
        headers,
        state.svid_validator.as_ref(),
        state.garage_validator.as_ref(),
    )
    .await
    {
        Ok(id) => id,
        Err(resp) => {
            let status = resp.status().as_u16();
            let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
            tracing::info!(
                request_id = %request_id,
                garage_id = "",
                provider = %info.name,
                mode = "passthrough",
                method = %method_str,
                path = %path,
                status = status,
                duration_ms = duration_ms,
                "request completed"
            );
            audit::log_ai_request_denied(
                &request_id,
                "",
                "auth failed",
                Some(&info.name),
                "passthrough",
                &start,
                headers,
            );
            let mut resp = resp;
            inject_moto_headers(&mut resp, &request_id, Some(&info.name));
            return resp;
        }
    };

    let result = forward_to_provider(
        &info,
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
        provider = %info.name,
        mode = "passthrough",
        method = %method_str,
        path = %path,
        status = status,
        upstream_status = result.upstream_status,
        duration_ms = duration_ms,
        "request completed"
    );

    // Emit audit event: provider_error for non-success upstream, ai_request for success.
    if result.response.status().is_success() {
        audit::log_ai_request(
            &request_id,
            &garage_id,
            &info.name,
            None,
            "passthrough",
            result.upstream_status,
            &start,
            headers,
            result.upstream_headers.as_ref(),
        );
    } else {
        audit::log_provider_error(
            &request_id,
            &garage_id,
            &info.name,
            None,
            "passthrough",
            result.upstream_status.unwrap_or(status),
            &start,
            headers,
            result.upstream_headers.as_ref(),
        );
    }

    let mut resp = result.response;
    inject_moto_headers(&mut resp, &request_id, Some(&info.name));
    resp
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

/// Result of resolving and translating a request.
struct ResolvedRequest {
    info: ProviderInfo,
    model: String,
    body: Body,
    /// Whether the original request had `stream: true`.
    is_streaming: bool,
}

/// Resolves the provider from the request body's `model` field and prepares
/// the body for forwarding, translating if needed (e.g., Anthropic).
fn resolve_and_translate(
    body: &Bytes,
    model_router: &ModelRouter,
) -> Result<ResolvedRequest, Box<Response>> {
    let parsed: Value = serde_json::from_slice(body).map_err(|_| {
        Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "invalid JSON in request body",
            "invalid_request_error",
        ))
    })?;

    let model = parsed
        .get("model")
        .and_then(|m| m.as_str())
        .map(String::from);

    let Some(model) = model else {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "missing or invalid 'model' field in request body",
            "invalid_request_error",
        )));
    };

    let Some(info) = model_router.resolve(&model) else {
        return Err(Box::new(error_response(
            StatusCode::BAD_REQUEST,
            "unknown model prefix, cannot determine provider",
            "invalid_request_error",
        )));
    };

    let is_streaming = parsed
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Translate request body for Anthropic; other providers use OpenAI format natively.
    let forwarded_body = if info.is_anthropic {
        let anthropic_value = anthropic_translate::translate_request(&parsed);
        Body::from(anthropic_value.to_string())
    } else {
        Body::from(body.clone())
    };

    Ok(ResolvedRequest {
        info,
        model,
        body: forwarded_body,
        is_streaming,
    })
}

/// Handler for `POST /v1/chat/completions` — unified endpoint.
///
/// Reads the request body, extracts the `model` field, and routes to the
/// correct provider. For `OpenAI` and Gemini, the request is forwarded directly
/// (both support OpenAI-compatible format). Anthropic requires translation.
#[allow(clippy::too_many_lines)]
pub async fn chat_completions<K: KeyStore, G: GarageValidator>(
    State(state): State<ProxyState<K, G>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let request_id = Uuid::now_v7();
    let start = Instant::now();
    let path = uri.path().to_string();

    let garage_id = match validate_garage_auth(
        &headers,
        state.svid_validator.as_ref(),
        state.garage_validator.as_ref(),
    )
    .await
    {
        Ok(id) => id,
        Err(resp) => {
            log_request(
                &request_id,
                "",
                None,
                "unified",
                "POST",
                &path,
                resp.status().as_u16(),
                None,
                &start,
            );
            audit::log_ai_request_denied(
                &request_id,
                "",
                "auth failed",
                None,
                "unified",
                &start,
                &headers,
            );
            let mut resp = resp;
            inject_moto_headers(&mut resp, &request_id, None);
            return resp;
        }
    };

    let resolved = match resolve_and_translate(&body, &state.model_router) {
        Ok(r) => r,
        Err(resp) => {
            log_request(
                &request_id,
                &garage_id,
                None,
                "unified",
                "POST",
                &path,
                resp.status().as_u16(),
                None,
                &start,
            );
            audit::log_ai_request_denied(
                &request_id,
                &garage_id,
                "invalid request",
                None,
                "unified",
                &start,
                &headers,
            );
            let mut resp = *resp;
            inject_moto_headers(&mut resp, &request_id, None);
            return resp;
        }
    };

    let info = resolved.info;
    let model = resolved.model;
    let is_streaming = resolved.is_streaming;
    let upstream_path = info.chat_path.clone();
    let provider_name = info.name.clone();

    // Forward the body to the upstream provider.
    let result = forward_to_provider(
        &info,
        Method::POST,
        &upstream_path,
        uri.query(),
        &headers,
        resolved.body,
        &state.client,
        state.key_store.as_ref(),
    )
    .await;

    let status = result.response.status().as_u16();
    log_request(
        &request_id,
        &garage_id,
        Some(&model),
        "unified",
        "POST",
        &path,
        status,
        result.upstream_status,
        &start,
    );

    // Emit audit event: provider_error for non-success upstream, ai_request for success.
    if result.response.status().is_success() {
        audit::log_ai_request(
            &request_id,
            &garage_id,
            &provider_name,
            Some(&model),
            "unified",
            result.upstream_status,
            &start,
            &headers,
            result.upstream_headers.as_ref(),
        );
    } else {
        audit::log_provider_error(
            &request_id,
            &garage_id,
            &provider_name,
            Some(&model),
            "unified",
            result.upstream_status.unwrap_or(status),
            &start,
            &headers,
            result.upstream_headers.as_ref(),
        );
    }

    // For Anthropic responses, translate back to OpenAI format.
    let mut resp = if info.is_anthropic && result.response.status().is_success() {
        if is_streaming {
            translate_anthropic_streaming_response(result.response)
        } else {
            translate_anthropic_response(result.response).await
        }
    } else {
        result.response
    };

    inject_moto_headers(&mut resp, &request_id, Some(&provider_name));
    resp
}

/// Buffers an Anthropic response body, translates it to `OpenAI` format, and returns a new response.
async fn translate_anthropic_response(response: Response) -> Response {
    let status = response.status();
    let headers = response.headers().clone();

    let body_bytes = match axum::body::to_bytes(response.into_body(), MAX_REQUEST_BODY_SIZE).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!(error = %e, "failed to buffer Anthropic response for translation");
            return error_response(
                StatusCode::BAD_GATEWAY,
                "failed to read upstream response",
                "server_error",
            );
        }
    };

    let Ok(anthropic_value) = serde_json::from_slice::<Value>(&body_bytes) else {
        tracing::error!("failed to parse Anthropic response as JSON for translation");
        return error_response(
            StatusCode::BAD_GATEWAY,
            "failed to parse upstream response",
            "server_error",
        );
    };

    let openai_value = anthropic_translate::translate_response(&anthropic_value);
    let translated_body = openai_value.to_string();

    let mut resp = Response::new(Body::from(translated_body));
    *resp.status_mut() = status;
    resp.headers_mut()
        .insert("content-type", HeaderValue::from_static("application/json"));
    // Preserve any extra headers from the original response (e.g., cache-control).
    for (name, value) in &headers {
        if name != "content-type" && name != "content-length" && name != "transfer-encoding" {
            resp.headers_mut().insert(name.clone(), value.clone());
        }
    }

    resp
}

/// Wraps an Anthropic streaming SSE response, translating each event to `OpenAI` chunk format.
///
/// Parses Anthropic SSE events (`message_start`, `content_block_delta`, `message_delta`, `message_stop`)
/// and emits the equivalent `OpenAI` `chat.completion.chunk` SSE events.
fn translate_anthropic_streaming_response(response: Response) -> Response {
    let status = response.status();
    let headers = response.headers().clone();

    let body_stream = response.into_body().into_data_stream();

    // Buffer partial SSE events across chunks, split on double-newline boundaries.
    let translated_stream = {
        let mut buffer = String::new();
        let mut translator = anthropic_translate::StreamingTranslator::new();
        body_stream.filter_map(move |chunk_result| {
            let output = match chunk_result {
                Ok(chunk) => {
                    let chunk_str = String::from_utf8_lossy(&chunk);
                    buffer.push_str(&chunk_str);

                    let mut translated_parts = Vec::new();

                    // Process all complete SSE events (delimited by \n\n).
                    while let Some(boundary) = buffer.find("\n\n") {
                        let event_block = buffer[..boundary].to_string();
                        buffer = buffer[boundary + 2..].to_string();

                        if let Some(event) = anthropic_translate::parse_sse_event(&event_block)
                            && let Some(translated) = translator.translate_event(&event)
                        {
                            translated_parts.push(translated);
                        }
                    }

                    if translated_parts.is_empty() {
                        None
                    } else {
                        let combined = translated_parts.join("");
                        Some(Ok(axum::body::Bytes::from(combined)))
                    }
                }
                Err(e) => Some(Err(e)),
            };
            std::future::ready(output)
        })
    };

    let body = Body::from_stream(translated_stream);

    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    // Set content-type to text/event-stream for OpenAI SSE format.
    resp.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("text/event-stream"),
    );
    // Preserve cache-control and other relevant headers.
    for (name, value) in &headers {
        if name != "content-type" && name != "content-length" && name != "transfer-encoding" {
            resp.headers_mut().insert(name.clone(), value.clone());
        }
    }

    resp
}

/// Emits a canonical log line for a unified endpoint request.
#[allow(clippy::too_many_arguments)]
fn log_request(
    request_id: &Uuid,
    garage_id: &str,
    model: Option<&str>,
    mode: &str,
    method: &str,
    path: &str,
    status: u16,
    upstream_status: Option<u16>,
    start: &Instant,
) {
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        request_id = %request_id,
        garage_id = %garage_id,
        model = model.unwrap_or(""),
        mode = mode,
        method = method,
        path = path,
        status = status,
        upstream_status = upstream_status,
        duration_ms = duration_ms,
        "request completed"
    );
}

/// Fetches the model list from a single provider's API.
///
/// For `OpenAI` and Gemini (which have `/v1/models`-compatible endpoints), fetches
/// from upstream. For Anthropic (no public models endpoint), returns a static
/// list of known models.
async fn fetch_provider_models<K: KeyStore>(
    provider: Provider,
    client: &Client,
    key_store: &K,
) -> Vec<Value> {
    let Some(api_key) = key_store.get_key(provider.secret_name()).await else {
        return Vec::new();
    };

    // Anthropic doesn't have a public /v1/models endpoint — return known models.
    if provider == Provider::Anthropic {
        return ANTHROPIC_KNOWN_MODELS
            .iter()
            .map(|id| {
                serde_json::json!({
                    "id": id,
                    "object": "model",
                    "owned_by": "anthropic",
                })
            })
            .collect();
    }

    // For OpenAI and Gemini, fetch from their models endpoint.
    let models_path = match provider {
        Provider::OpenAi => "v1/models",
        Provider::Gemini => "v1beta/openai/models",
        Provider::Anthropic => unreachable!(),
    };

    let url = format!("{}{}", provider.upstream_base(), models_path);
    let req = client.get(&url).header(
        provider.auth_header(),
        provider.auth_value(api_key.expose_secret()),
    );

    let resp = match tokio::time::timeout(FIRST_BYTE_TIMEOUT, req.send()).await {
        Ok(Ok(r)) if r.status().is_success() => r,
        Ok(Ok(r)) => {
            tracing::warn!(provider = %provider, status = %r.status(), "failed to fetch models from provider");
            return Vec::new();
        }
        Ok(Err(e)) => {
            tracing::warn!(provider = %provider, error = %e, "failed to fetch models from provider");
            return Vec::new();
        }
        Err(_) => {
            tracing::warn!(provider = %provider, "timeout fetching models from provider");
            return Vec::new();
        }
    };

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(provider = %provider, error = %e, "failed to parse models response");
            return Vec::new();
        }
    };

    // OpenAI format: {"data": [{"id": "...", "object": "model", ...}]}
    body.get("data")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Handler for `GET /v1/models` — returns merged model list from all configured providers.
pub async fn list_models<K: KeyStore, G: GarageValidator>(
    State(state): State<ProxyState<K, G>>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let request_id = Uuid::now_v7();
    let start = Instant::now();
    let path = uri.path().to_string();

    let garage_id = match validate_garage_auth(
        &headers,
        state.svid_validator.as_ref(),
        state.garage_validator.as_ref(),
    )
    .await
    {
        Ok(id) => id,
        Err(resp) => {
            log_request(
                &request_id,
                "",
                None,
                "unified",
                "GET",
                &path,
                resp.status().as_u16(),
                None,
                &start,
            );
            let mut resp = resp;
            inject_moto_headers(&mut resp, &request_id, None);
            return resp;
        }
    };

    // Fetch models from all providers in parallel.
    let (anthropic, openai, gemini) = tokio::join!(
        fetch_provider_models(Provider::Anthropic, &state.client, state.key_store.as_ref()),
        fetch_provider_models(Provider::OpenAi, &state.client, state.key_store.as_ref()),
        fetch_provider_models(Provider::Gemini, &state.client, state.key_store.as_ref()),
    );

    let mut all_models = Vec::new();
    all_models.extend(anthropic);
    all_models.extend(openai);
    all_models.extend(gemini);

    let response_body = serde_json::json!({
        "object": "list",
        "data": all_models,
    });

    let status = StatusCode::OK;
    log_request(
        &request_id,
        &garage_id,
        None,
        "unified",
        "GET",
        &path,
        status.as_u16(),
        None,
        &start,
    );

    let mut resp: Response = (
        status,
        [(
            axum::http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )],
        response_body.to_string(),
    )
        .into_response();
    inject_moto_headers(&mut resp, &request_id, None);
    resp
}

/// Builds the proxy router with passthrough routes and garage auth.
pub fn proxy_router<K: KeyStore + 'static, G: GarageValidator + 'static>(
    client: Client,
    key_store: K,
    svid_validator: SvidValidator,
    garage_validator: G,
    model_router: ModelRouter,
) -> axum::Router {
    let state = ProxyState {
        client,
        key_store: Arc::new(key_store),
        svid_validator: Arc::new(svid_validator),
        garage_validator: Arc::new(garage_validator),
        model_router,
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
        .route("/v1/models", axum::routing::get(list_models::<K, G>))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_SIZE))
        .with_state(state)
}

/// Maximum size for buffering upstream error response bodies (1 MB).
const MAX_ERROR_BODY_SIZE: usize = 1024 * 1024;

/// Buffers an upstream error response, extracts the error message, scrubs
/// API key material, and wraps it in `OpenAI` error format.
///
/// Raw upstream error bodies are never forwarded directly to garages.
async fn sanitize_upstream_error(
    provider: &str,
    resp: reqwest::Response,
    status: StatusCode,
) -> Response {
    let error_type = if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        "authentication_error"
    } else if status.is_client_error() {
        "invalid_request_error"
    } else {
        "server_error"
    };

    // Try to extract a useful error message from the upstream response body.
    let message = match resp.bytes().await {
        Ok(body) if !body.is_empty() && body.len() <= MAX_ERROR_BODY_SIZE => {
            extract_upstream_error_message(provider, &body)
        }
        _ => format!("{provider} returned {status}"),
    };

    let scrubbed = scrub_api_keys(&message);
    error_response(status, &scrubbed, error_type)
}

/// Extracts a human-readable error message from an upstream provider's error body.
///
/// Tries to parse as JSON and extract the message from known error formats
/// (`OpenAI`, Anthropic). Falls back to a generic message if parsing fails.
fn extract_upstream_error_message(provider: &str, body: &[u8]) -> String {
    let Ok(json) = serde_json::from_slice::<Value>(body) else {
        return format!("{provider} error");
    };

    // OpenAI format: {"error": {"message": "..."}}
    if let Some(msg) = json
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
    {
        return msg.to_string();
    }

    // Anthropic format: {"error": {"message": "..."}} or {"message": "..."}
    if let Some(msg) = json.get("message").and_then(|m| m.as_str()) {
        return msg.to_string();
    }

    format!("{provider} error")
}

/// Scrubs API key material from a string, replacing recognized key patterns
/// with `[REDACTED]`.
///
/// Recognized patterns:
/// - `sk-ant-*` (Anthropic API keys)
/// - `sk-proj-*` (`OpenAI` project-scoped keys)
/// - `sk-` followed by 20+ alphanumeric/dash/underscore characters
/// - `AIza*` (Google API keys)
#[must_use]
pub fn scrub_api_keys(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if let Some(skip) = detect_api_key(&chars, i) {
            result.push_str("[REDACTED]");
            i += skip;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }

    result
}

/// Detects an API key pattern starting at position `i` in the char slice.
/// Returns `Some(length)` if a key pattern is detected, `None` otherwise.
fn detect_api_key(chars: &[char], i: usize) -> Option<usize> {
    let remaining = &chars[i..];

    // Check for sk-ant- (Anthropic)
    if starts_with_str(remaining, "sk-ant-") {
        return Some(key_token_len(remaining));
    }

    // Check for sk-proj- (OpenAI project-scoped)
    if starts_with_str(remaining, "sk-proj-") {
        return Some(key_token_len(remaining));
    }

    // Check for sk- followed by 20+ key characters (generic OpenAI)
    if starts_with_str(remaining, "sk-") {
        let len = key_token_len(remaining);
        if len >= 23 {
            // sk- + at least 20 chars
            return Some(len);
        }
    }

    // Check for AIza (Google API keys)
    if starts_with_str(remaining, "AIza") {
        let len = key_token_len(remaining);
        if len >= 20 {
            return Some(len);
        }
    }

    None
}

/// Checks if a char slice starts with the given string.
fn starts_with_str(chars: &[char], s: &str) -> bool {
    let prefix: Vec<char> = s.chars().collect();
    if chars.len() < prefix.len() {
        return false;
    }
    chars[..prefix.len()] == prefix[..]
}

/// Returns the length of a contiguous API key token (alphanumeric, dash, underscore).
fn key_token_len(chars: &[char]) -> usize {
    chars
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '-' || **c == '_')
        .count()
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
