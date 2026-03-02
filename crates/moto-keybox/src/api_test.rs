//! Handler-level tests for the keybox API auth matrix enforcement.
//!
//! Tests that the in-memory API enforces the endpoint authorization matrix:
//! - `set_secret` and `delete_secret` require service token (deny SVID with 403)
//! - `get_secret` and `list_secrets` accept both service token and SVID
//! - `get_audit_logs` requires service token (deny SVID with 403)

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::api::{AppState, error_codes, router};
use crate::envelope::MasterKey;
use crate::svid::{SvidIssuer, SvidValidator};
use crate::types::SpiffeId;

const SERVICE_TOKEN: &str = "test-service-token";

/// Creates a test router with a service token and returns (router, `svid_token`).
///
/// The SVID is for a garage principal with id "test-garage".
fn test_app() -> (axum::Router, String) {
    let master_key = MasterKey::generate();
    let signing_key = SvidIssuer::generate_key();
    let issuer = SvidIssuer::new(signing_key);
    let validator = SvidValidator::new(issuer.verifying_key());

    let state = AppState::new(master_key, issuer.clone(), validator, "moto-club")
        .with_service_token(SERVICE_TOKEN);

    let svid_token = issuer.issue(&SpiffeId::garage("test-garage")).unwrap();

    (router(state), svid_token)
}

/// Seeds a global secret via service token, returns the cloned router.
async fn seed_global_secret(app: axum::Router) -> axum::Router {
    let req = Request::builder()
        .method("POST")
        .uri("/secrets/global/test-secret")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"value":"dGVzdC12YWx1ZQ=="}"#))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    app
}

/// Extracts the JSON error code from a response body.
async fn error_code(resp: axum::http::Response<Body>) -> String {
    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    json["error"]["code"].as_str().unwrap().to_string()
}

// ============================================================================
// Auth matrix: endpoints that DENY SVID tokens
// ============================================================================

#[tokio::test]
async fn set_secret_with_svid_returns_403_forbidden() {
    let (app, svid_token) = test_app();

    let req = Request::builder()
        .method("POST")
        .uri("/secrets/global/test-secret")
        .header("authorization", format!("Bearer {svid_token}"))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"value":"dGVzdA=="}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, error_codes::FORBIDDEN);
}

#[tokio::test]
async fn delete_secret_with_svid_returns_403_forbidden() {
    let (app, svid_token) = test_app();

    let req = Request::builder()
        .method("DELETE")
        .uri("/secrets/global/test-secret")
        .header("authorization", format!("Bearer {svid_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, error_codes::FORBIDDEN);
}

#[tokio::test]
async fn get_audit_logs_with_svid_returns_403_forbidden() {
    let (app, svid_token) = test_app();

    let req = Request::builder()
        .method("GET")
        .uri("/audit/logs")
        .header("authorization", format!("Bearer {svid_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, error_codes::FORBIDDEN);
}

// ============================================================================
// Auth matrix: endpoints that ACCEPT service token
// ============================================================================

#[tokio::test]
async fn get_secret_succeeds_with_service_token() {
    let (app, _) = test_app();
    let app = seed_global_secret(app).await;

    let req = Request::builder()
        .method("GET")
        .uri("/secrets/global/test-secret")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn list_secrets_succeeds_with_service_token() {
    let (app, _) = test_app();

    let req = Request::builder()
        .method("GET")
        .uri("/secrets/global")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn get_audit_logs_succeeds_with_service_token() {
    let (app, _) = test_app();

    let req = Request::builder()
        .method("GET")
        .uri("/audit/logs")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ============================================================================
// Auth matrix: endpoints that ACCEPT SVID tokens
// ============================================================================

#[tokio::test]
async fn get_secret_succeeds_with_valid_svid() {
    let (app, svid_token) = test_app();
    // Seed a global secret (garages can read global secrets per ABAC)
    let app = seed_global_secret(app).await;

    let req = Request::builder()
        .method("GET")
        .uri("/secrets/global/test-secret")
        .header("authorization", format!("Bearer {svid_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn list_secrets_succeeds_with_valid_svid() {
    let (app, svid_token) = test_app();

    let req = Request::builder()
        .method("GET")
        .uri("/secrets/global")
        .header("authorization", format!("Bearer {svid_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
