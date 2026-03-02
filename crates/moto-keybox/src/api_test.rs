//! Handler-level tests for the keybox API auth matrix enforcement and DEK rotation.
//!
//! Tests that the in-memory API enforces the endpoint authorization matrix:
//! - `set_secret` and `delete_secret` require service token (deny SVID with 403)
//! - `get_secret` and `list_secrets` accept both service token and SVID
//! - `get_audit_logs` requires service token (deny SVID with 403)
//! - `rotate_dek` requires service token (deny SVID with 403)
//! - DEK rotation: succeeds with service token, 404 for missing secret, value unchanged,
//!   version incremented, audit event logged

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use crate::api::{AppState, RotateDekResponse, error_codes, router};
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

// ============================================================================
// DEK rotation tests
// ============================================================================

/// Extracts JSON body from a response as `serde_json::Value`.
async fn json_body(resp: axum::http::Response<Body>) -> serde_json::Value {
    let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn rotate_dek_with_svid_returns_403_forbidden() {
    let (app, svid_token) = test_app();

    let req = Request::builder()
        .method("POST")
        .uri("/admin/rotate-dek/test-secret?scope=global")
        .header("authorization", format!("Bearer {svid_token}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert_eq!(error_code(resp).await, error_codes::FORBIDDEN);
}

#[tokio::test]
async fn rotate_dek_with_service_token_succeeds() {
    let (app, _) = test_app();
    let app = seed_global_secret(app).await;

    let req = Request::builder()
        .method("POST")
        .uri("/admin/rotate-dek/test-secret?scope=global")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = json_body(resp).await;
    let rotate_resp: RotateDekResponse = serde_json::from_value(body).unwrap();
    assert_eq!(rotate_resp.name, "test-secret");
    assert_eq!(rotate_resp.version, 2); // Incremented from initial version 1
}

#[tokio::test]
async fn rotate_dek_for_nonexistent_secret_returns_404() {
    let (app, _) = test_app();

    let req = Request::builder()
        .method("POST")
        .uri("/admin/rotate-dek/no-such-secret?scope=global")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(error_code(resp).await, error_codes::SECRET_NOT_FOUND);
}

#[tokio::test]
async fn secret_value_readable_after_dek_rotation() {
    let (app, _) = test_app();
    let app = seed_global_secret(app).await;

    // Rotate the DEK
    let req = Request::builder()
        .method("POST")
        .uri("/admin/rotate-dek/test-secret?scope=global")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Read the secret — value should be unchanged (base64 "dGVzdC12YWx1ZQ==" = "test-value")
    let req = Request::builder()
        .method("GET")
        .uri("/secrets/global/test-secret")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = json_body(resp).await;
    assert_eq!(body["value"].as_str().unwrap(), "dGVzdC12YWx1ZQ==");
}

#[tokio::test]
async fn dek_rotation_increments_version() {
    let (app, _) = test_app();
    let app = seed_global_secret(app).await;

    // First rotation: version 1 → 2
    let req = Request::builder()
        .method("POST")
        .uri("/admin/rotate-dek/test-secret?scope=global")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["version"].as_u64().unwrap(), 2);

    // Second rotation: version 2 → 3
    let req = Request::builder()
        .method("POST")
        .uri("/admin/rotate-dek/test-secret?scope=global")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = json_body(resp).await;
    assert_eq!(body["version"].as_u64().unwrap(), 3);
}

#[tokio::test]
async fn dek_rotation_logs_audit_event() {
    let (app, _) = test_app();
    let app = seed_global_secret(app).await;

    // Rotate the DEK
    let req = Request::builder()
        .method("POST")
        .uri("/admin/rotate-dek/test-secret?scope=global")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Check audit logs for dek_rotated event
    let req = Request::builder()
        .method("GET")
        .uri("/audit/logs")
        .header("authorization", format!("Bearer {SERVICE_TOKEN}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = json_body(resp).await;
    let entries = body["entries"].as_array().unwrap();
    let has_dek_rotated = entries
        .iter()
        .any(|e| e["event_type"].as_str() == Some("dek_rotated"));
    assert!(
        has_dek_rotated,
        "Expected dek_rotated audit event, got: {entries:?}"
    );
}
