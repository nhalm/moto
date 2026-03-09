use axum::http::{HeaderMap, StatusCode};

use super::validate_service_token;

#[test]
fn validate_service_token_missing_header() {
    let headers = HeaderMap::new();
    let token = Some("test-token".to_string());
    let result = validate_service_token(&headers, token.as_ref());
    assert!(result.is_err());
    let (status, _) = result.unwrap_err();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[test]
fn validate_service_token_invalid_format() {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Basic abc123".parse().unwrap());
    let token = Some("test-token".to_string());
    let result = validate_service_token(&headers, token.as_ref());
    assert!(result.is_err());
    let (status, _) = result.unwrap_err();
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[test]
fn validate_service_token_not_configured() {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer some-token".parse().unwrap());
    let result = validate_service_token(&headers, None);
    assert!(result.is_err());
    let (status, _) = result.unwrap_err();
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn validate_service_token_wrong_token() {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer wrong-token".parse().unwrap());
    let token = Some("correct-token".to_string());
    let result = validate_service_token(&headers, token.as_ref());
    assert!(result.is_err());
    let (status, _) = result.unwrap_err();
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[test]
fn validate_service_token_correct_token() {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "Bearer my-secret-token".parse().unwrap());
    let token = Some("my-secret-token".to_string());
    let result = validate_service_token(&headers, token.as_ref());
    assert!(result.is_ok());
}

#[test]
fn validate_service_token_case_insensitive_bearer() {
    let mut headers = HeaderMap::new();
    headers.insert("authorization", "bearer my-secret-token".parse().unwrap());
    let token = Some("my-secret-token".to_string());
    let result = validate_service_token(&headers, token.as_ref());
    assert!(result.is_ok());
}
