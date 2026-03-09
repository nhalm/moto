use axum::body::Body;
use axum::extract::Request;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::config::PrincipalType;
use crate::layer::extract_principal;

/// Helper to build a JWT token (unsigned) with given claims.
fn make_jwt(claims: &serde_json::Value) -> String {
    let header = URL_SAFE_NO_PAD.encode(b"{}");
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
    format!("{header}.{payload}.sig")
}

fn request_with_auth(bearer: &str) -> Request<Body> {
    Request::builder()
        .header("authorization", format!("Bearer {bearer}"))
        .body(Body::empty())
        .unwrap()
}

fn request_with_api_key(key: &str) -> Request<Body> {
    Request::builder()
        .header("x-api-key", key)
        .body(Body::empty())
        .unwrap()
}

fn request_with_ip(ip: &str) -> Request<Body> {
    Request::builder()
        .header("x-forwarded-for", ip)
        .body(Body::empty())
        .unwrap()
}

fn empty_request() -> Request<Body> {
    Request::builder().body(Body::empty()).unwrap()
}

#[test]
fn jwt_garage_from_authorization() {
    let token = make_jwt(&serde_json::json!({
        "principal_type": "garage",
        "principal_id": "garage-abc123"
    }));
    let req = request_with_auth(&token);

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Garage);
    assert_eq!(principal.key, "garage-abc123");
}

#[test]
fn jwt_bike_from_api_key() {
    let token = make_jwt(&serde_json::json!({
        "principal_type": "bike",
        "principal_id": "bike-engine-1"
    }));
    let req = request_with_api_key(&token);

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Bike);
    assert_eq!(principal.key, "bike-engine-1");
}

#[test]
fn service_token_detection() {
    let req = request_with_auth("deadbeef1234");

    let principal = extract_principal(&req, Some("deadbeef1234"));
    assert_eq!(principal.principal_type, PrincipalType::Service);
    assert_eq!(principal.key, "service-token");
}

#[test]
fn service_token_from_api_key() {
    let req = request_with_api_key("my-service-token");

    let principal = extract_principal(&req, Some("my-service-token"));
    assert_eq!(principal.principal_type, PrincipalType::Service);
    assert_eq!(principal.key, "service-token");
}

#[test]
fn unknown_fallback_with_ip() {
    let req = request_with_ip("10.0.0.1");

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Unknown);
    assert_eq!(principal.key, "10.0.0.1");
}

#[test]
fn unknown_fallback_no_headers() {
    let req = empty_request();

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Unknown);
    assert_eq!(principal.key, "unknown");
}

#[test]
fn malformed_jwt_invalid_base64_falls_to_service_token() {
    let req = request_with_auth("not.valid-base64!!!.sig");

    let principal = extract_principal(&req, Some("not.valid-base64!!!.sig"));
    assert_eq!(principal.principal_type, PrincipalType::Service);
    assert_eq!(principal.key, "service-token");
}

#[test]
fn malformed_jwt_missing_claims_falls_to_unknown() {
    let token = make_jwt(&serde_json::json!({"sub": "user1"}));
    let req = request_with_auth(&token);

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Unknown);
}

#[test]
fn authorization_takes_precedence_over_api_key() {
    let garage_jwt = make_jwt(&serde_json::json!({
        "principal_type": "garage",
        "principal_id": "garage-1"
    }));
    let bike_jwt = make_jwt(&serde_json::json!({
        "principal_type": "bike",
        "principal_id": "bike-1"
    }));
    let req = Request::builder()
        .header("authorization", format!("Bearer {garage_jwt}"))
        .header("x-api-key", &bike_jwt)
        .body(Body::empty())
        .unwrap();

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Garage);
    assert_eq!(principal.key, "garage-1");
}

#[test]
fn unknown_principal_type_in_jwt_falls_through() {
    let token = make_jwt(&serde_json::json!({
        "principal_type": "admin",
        "principal_id": "admin-1"
    }));
    let req = request_with_auth(&token);

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Unknown);
}

#[test]
fn non_bearer_auth_ignored() {
    let req = Request::builder()
        .header("authorization", "Basic dXNlcjpwYXNz")
        .header("x-forwarded-for", "10.0.0.5")
        .body(Body::empty())
        .unwrap();

    let principal = extract_principal(&req, None);
    assert_eq!(principal.principal_type, PrincipalType::Unknown);
    assert_eq!(principal.key, "10.0.0.5");
}
