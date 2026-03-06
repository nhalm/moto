//! Core proxy logic — forwards requests to upstream AI providers.

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use reqwest::Client;

use crate::provider::Provider;

/// Shared state for proxy handlers.
#[derive(Clone)]
pub struct ProxyState {
    /// HTTP client for upstream requests.
    pub client: Client,
}

/// Forwards a request to the given provider's upstream, rewriting the path.
///
/// `remaining_path` is the path after the `/passthrough/{provider}/` prefix.
/// Auth injection is not handled here — it will be added in the keybox integration work item.
pub async fn forward_to_provider(
    provider: Provider,
    method: Method,
    remaining_path: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Body,
    client: &Client,
) -> Response {
    let base = provider.upstream_base();
    let mut upstream_url = format!("{base}{remaining_path}");
    if let Some(q) = query {
        upstream_url.push('?');
        upstream_url.push_str(q);
    }

    // Build the upstream request, forwarding Content-Type and Accept headers.
    let mut req = client.request(reqwest_method(&method), &upstream_url);

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

    // Stream the body through via reqwest's body wrapper.
    req = req.body(reqwest::Body::wrap_stream(body.into_data_stream()));

    let upstream_resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(provider = %provider, error = %e, "upstream request failed");
            return error_response(
                StatusCode::BAD_GATEWAY,
                &format!("upstream error: {provider}"),
                "server_error",
            );
        }
    };

    // Convert upstream response back to axum Response, streaming the body.
    let status =
        StatusCode::from_u16(upstream_resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut response_headers = HeaderMap::new();

    // Forward Content-Type from upstream.
    if let Some(ct) = upstream_resp.headers().get("content-type") {
        response_headers.insert("content-type", ct.clone());
    }
    // Forward Transfer-Encoding for streaming.
    if let Some(te) = upstream_resp.headers().get("transfer-encoding") {
        response_headers.insert("transfer-encoding", te.clone());
    }

    let body = Body::from_stream(upstream_resp.bytes_stream());

    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    *resp.headers_mut() = response_headers;

    resp
}

/// Handler for `/passthrough/anthropic/*path`.
pub async fn passthrough_anthropic(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(remaining): Path<String>,
    body: Body,
) -> Response {
    forward_to_provider(
        Provider::Anthropic,
        method,
        &remaining,
        uri.query(),
        &headers,
        body,
        &state.client,
    )
    .await
}

/// Handler for `/passthrough/openai/*path`.
pub async fn passthrough_openai(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(remaining): Path<String>,
    body: Body,
) -> Response {
    forward_to_provider(
        Provider::OpenAi,
        method,
        &remaining,
        uri.query(),
        &headers,
        body,
        &state.client,
    )
    .await
}

/// Handler for `/passthrough/gemini/*path`.
pub async fn passthrough_gemini(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    Path(remaining): Path<String>,
    body: Body,
) -> Response {
    forward_to_provider(
        Provider::Gemini,
        method,
        &remaining,
        uri.query(),
        &headers,
        body,
        &state.client,
    )
    .await
}

/// Builds the proxy router with passthrough routes.
pub fn proxy_router(client: Client) -> axum::Router {
    let state = ProxyState { client };
    axum::Router::new()
        .route(
            "/passthrough/anthropic/{*path}",
            axum::routing::any(passthrough_anthropic),
        )
        .route(
            "/passthrough/openai/{*path}",
            axum::routing::any(passthrough_openai),
        )
        .route(
            "/passthrough/gemini/{*path}",
            axum::routing::any(passthrough_gemini),
        )
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
