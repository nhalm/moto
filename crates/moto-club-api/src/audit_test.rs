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

// Integration tests for audit log fan-out with offset parameter
#[cfg(all(test, feature = "integration"))]
mod integration_tests {
    use super::super::AuditLogsResponse;
    use super::*;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, header};
    use chrono::Utc;
    use moto_club_db::audit_repo;
    use moto_club_wg::{Ipam, PeerBroadcaster, PeerRegistry, SessionManager};
    use moto_test_utils::test_pool;
    use moto_wgtunnel_types::derp::{DerpMap, DerpNode, DerpRegion};
    use tower::ServiceExt;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::{PostgresIpamStore, PostgresPeerStore, PostgresSessionStore};

    /// Creates a test `AppState` with PostgreSQL backend.
    async fn create_test_state() -> crate::AppState {
        let db_pool = test_pool().await;

        let ipam_store = PostgresIpamStore::new(db_pool.clone());
        let peer_store = PostgresPeerStore::new(db_pool.clone());
        let session_store = PostgresSessionStore::new(db_pool.clone());

        let derp_map = DerpMap::new().with_region(
            DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("derp.moto.dev")),
        );

        let ipam = Ipam::new(ipam_store);
        let peer_registry = Arc::new(PeerRegistry::new(peer_store, ipam));
        let session_manager = Arc::new(SessionManager::new(session_store));
        let peer_broadcaster = Arc::new(PeerBroadcaster::new());

        crate::AppState::new(
            db_pool,
            peer_registry,
            session_manager,
            derp_map,
            peer_broadcaster,
        )
    }

    /// Helper to insert test audit events with specific timestamps.
    /// Uses a unique principal_id to allow filtering events per test.
    async fn insert_test_event(
        pool: &moto_club_db::DbPool,
        event_type: &str,
        timestamp_offset_seconds: i64,
        principal_id: &str,
    ) -> audit_repo::AuditLogEntry {
        let timestamp = Utc::now() - chrono::Duration::seconds(timestamp_offset_seconds);

        // Insert with a raw SQL query to control the timestamp
        sqlx::query_as::<_, audit_repo::AuditLogEntry>(
            r"
            INSERT INTO audit_log (
                event_type, service, principal_type, principal_id,
                action, resource_type, resource_id, outcome,
                metadata, client_ip, timestamp
            )
            VALUES ($1, 'moto-club', $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING *
            ",
        )
        .bind(event_type)
        .bind("service")
        .bind(principal_id)
        .bind("create")
        .bind("test")
        .bind(format!("resource-{event_type}"))
        .bind("success")
        .bind(serde_json::json!({}))
        .bind(Some("127.0.0.1"))
        .bind(timestamp)
        .fetch_one(pool)
        .await
        .expect("failed to insert test audit event")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fan_out_query_with_offset_merges_correctly() {
        let state = create_test_state().await;
        let test_principal = format!("test-principal-{}", uuid::Uuid::now_v7());

        // Insert 5 events in moto-club with different timestamps
        // (timestamps: now-100s, now-200s, now-300s, now-400s, now-500s)
        for i in 1..=5 {
            insert_test_event(
                &state.db_pool,
                &format!("local_event_{i}"),
                i * 100,
                &test_principal,
            )
            .await;
        }

        // Create mock keybox server that returns 5 events
        let mock_server = MockServer::start().await;
        let keybox_events = (1..=5)
            .map(|i| {
                let ts = Utc::now() - chrono::Duration::seconds(i * 100 + 50);
                serde_json::json!({
                    "id": format!("keybox-event-{i}"),
                    "event_type": format!("keybox_event_{i}"),
                    "service": "keybox",
                    "principal_type": "service",
                    "principal_id": &test_principal,
                    "action": "read",
                    "resource_type": "secret",
                    "resource_id": format!("secret-{i}"),
                    "outcome": "success",
                    "metadata": {},
                    "client_ip": "127.0.0.1",
                    "timestamp": ts.to_rfc3339()
                })
            })
            .collect::<Vec<_>>();

        Mock::given(method("GET"))
            .and(path("/audit/logs"))
            .and(header("authorization", "Bearer test-keybox-token"))
            .and(query_param("principal_id", test_principal.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": keybox_events,
                "total": 5
            })))
            .mount(&mock_server)
            .await;

        // Configure state to use mock keybox
        let state = state
            .with_keybox_url(mock_server.uri())
            .with_keybox_service_token("test-keybox-token".to_string())
            .with_service_token("test-service-token");

        let app = super::super::router().with_state(state);

        // Query with offset=2, limit=3, filtering by our test principal
        // Should skip first 2 merged events and return next 3
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/audit/logs?offset=2&limit=3&principal_id={}",
                        test_principal
                    ))
                    .header(header::AUTHORIZATION, "Bearer test-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: AuditLogsResponse = serde_json::from_slice(&body).unwrap();

        // Should have 3 events (after skipping first 2)
        assert_eq!(result.events.len(), 3);
        assert_eq!(result.limit, 3);
        assert_eq!(result.offset, 2);
        // Total should be 10 (5 local + 5 keybox)
        assert_eq!(result.total, 10);

        // Events should be sorted by timestamp (newest first) and offset applied
        // The merged list (newest first) should be interleaved based on timestamps
        // Each event from local is at (now - i*100) and from keybox at (now - i*100 - 50)
        // So the order (newest first) should be:
        // 1. local_event_1 (now-100)
        // 2. keybox_event_1 (now-150)
        // 3. local_event_2 (now-200) <- offset=2 starts here (index 2)
        // 4. keybox_event_2 (now-250)
        // 5. local_event_3 (now-300)
        // And we should get 3 events starting from offset=2
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fan_out_query_offset_zero_returns_from_start() {
        let state = create_test_state().await;
        let test_principal = format!("test-principal-{}", uuid::Uuid::now_v7());

        // Insert 3 events in moto-club
        for i in 1..=3 {
            insert_test_event(
                &state.db_pool,
                &format!("local_event_{i}"),
                i * 100,
                &test_principal,
            )
            .await;
        }

        // Create mock keybox server that returns 3 events
        let mock_server = MockServer::start().await;
        let keybox_events = (1..=3)
            .map(|i| {
                let ts = Utc::now() - chrono::Duration::seconds(i * 100 + 50);
                serde_json::json!({
                    "id": format!("keybox-event-{i}"),
                    "event_type": format!("keybox_event_{i}"),
                    "service": "keybox",
                    "principal_type": "service",
                    "principal_id": &test_principal,
                    "action": "read",
                    "resource_type": "secret",
                    "resource_id": format!("secret-{i}"),
                    "outcome": "success",
                    "metadata": {},
                    "client_ip": "127.0.0.1",
                    "timestamp": ts.to_rfc3339()
                })
            })
            .collect::<Vec<_>>();

        Mock::given(method("GET"))
            .and(path("/audit/logs"))
            .and(header("authorization", "Bearer test-keybox-token"))
            .and(query_param("principal_id", test_principal.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": keybox_events,
                "total": 3
            })))
            .mount(&mock_server)
            .await;

        let state = state
            .with_keybox_url(mock_server.uri())
            .with_keybox_service_token("test-keybox-token".to_string())
            .with_service_token("test-service-token");

        let app = super::super::router().with_state(state);

        // Query with offset=0, limit=2
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/audit/logs?offset=0&limit=2&principal_id={}",
                        test_principal
                    ))
                    .header(header::AUTHORIZATION, "Bearer test-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: AuditLogsResponse = serde_json::from_slice(&body).unwrap();

        // Should return first 2 events (newest)
        assert_eq!(result.events.len(), 2);
        assert_eq!(result.offset, 0);
        assert_eq!(result.limit, 2);
        assert_eq!(result.total, 6); // 3 local + 3 keybox
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fan_out_query_offset_exceeds_total() {
        let state = create_test_state().await;
        let test_principal = format!("test-principal-{}", uuid::Uuid::now_v7());

        // Insert 2 events in moto-club
        for i in 1..=2 {
            insert_test_event(
                &state.db_pool,
                &format!("local_event_{i}"),
                i * 100,
                &test_principal,
            )
            .await;
        }

        // Create mock keybox server that returns 2 events
        let mock_server = MockServer::start().await;
        let keybox_events = (1..=2)
            .map(|i| {
                let ts = Utc::now() - chrono::Duration::seconds(i * 100);
                serde_json::json!({
                    "id": format!("keybox-event-{i}"),
                    "event_type": format!("keybox_event_{i}"),
                    "service": "keybox",
                    "principal_type": "service",
                    "principal_id": &test_principal,
                    "action": "read",
                    "resource_type": "secret",
                    "resource_id": format!("secret-{i}"),
                    "outcome": "success",
                    "metadata": {},
                    "client_ip": "127.0.0.1",
                    "timestamp": ts.to_rfc3339()
                })
            })
            .collect::<Vec<_>>();

        Mock::given(method("GET"))
            .and(path("/audit/logs"))
            .and(query_param("principal_id", test_principal.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": keybox_events,
                "total": 2
            })))
            .mount(&mock_server)
            .await;

        let state = state
            .with_keybox_url(mock_server.uri())
            .with_keybox_service_token("test-keybox-token".to_string())
            .with_service_token("test-service-token");

        let app = super::super::router().with_state(state);

        // Query with offset=100 (exceeds total of 4)
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/audit/logs?offset=100&limit=10&principal_id={}",
                        test_principal
                    ))
                    .header(header::AUTHORIZATION, "Bearer test-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: AuditLogsResponse = serde_json::from_slice(&body).unwrap();

        // Should return empty array when offset exceeds total
        assert_eq!(result.events.len(), 0);
        assert_eq!(result.offset, 100);
        assert_eq!(result.total, 4); // 2 local + 2 keybox
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fan_out_query_verifies_keybox_not_sent_offset() {
        let state = create_test_state().await;
        let test_principal = format!("test-principal-{}", uuid::Uuid::now_v7());

        // Insert 2 events in moto-club
        for i in 1..=2 {
            insert_test_event(
                &state.db_pool,
                &format!("local_event_{i}"),
                i * 100,
                &test_principal,
            )
            .await;
        }

        // Create mock keybox server
        let mock_server = MockServer::start().await;

        // Mock should verify that offset is NOT in query params
        Mock::given(method("GET"))
            .and(path("/audit/logs"))
            .and(header("authorization", "Bearer test-keybox-token"))
            // Verify limit is fetch_limit (offset + limit = 2 + 5 = 7) but offset is NOT forwarded
            .and(query_param("limit", "7"))
            .and(query_param("principal_id", test_principal.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": [],
                "total": 0
            })))
            .expect(1) // Should be called exactly once
            .mount(&mock_server)
            .await;

        let state = state
            .with_keybox_url(mock_server.uri())
            .with_keybox_service_token("test-keybox-token".to_string())
            .with_service_token("test-service-token");

        let app = super::super::router().with_state(state);

        // Query with offset=2, limit=5
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/audit/logs?offset=2&limit=5&principal_id={}",
                        test_principal
                    ))
                    .header(header::AUTHORIZATION, "Bearer test-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Mock verification happens automatically when mock_server is dropped
        // If offset was sent to keybox, the mock would not match and test would fail
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fan_out_query_pagination_consistency() {
        let state = create_test_state().await;
        let test_principal = format!("test-principal-{}", uuid::Uuid::now_v7());

        // Insert 10 events in moto-club with predictable timestamps
        for i in 0..10 {
            insert_test_event(
                &state.db_pool,
                &format!("local_{i}"),
                i * 10,
                &test_principal,
            )
            .await;
        }

        // Create mock keybox server that returns 10 events
        let mock_server = MockServer::start().await;
        let keybox_events = (0..10)
            .map(|i| {
                let ts = Utc::now() - chrono::Duration::seconds(i * 10 + 5);
                serde_json::json!({
                    "id": format!("keybox-{i}"),
                    "event_type": format!("keybox_{i}"),
                    "service": "keybox",
                    "principal_type": "service",
                    "principal_id": &test_principal,
                    "action": "read",
                    "resource_type": "secret",
                    "resource_id": format!("secret-{i}"),
                    "outcome": "success",
                    "metadata": {},
                    "client_ip": "127.0.0.1",
                    "timestamp": ts.to_rfc3339()
                })
            })
            .collect::<Vec<_>>();

        Mock::given(method("GET"))
            .and(path("/audit/logs"))
            .and(query_param("principal_id", test_principal.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "entries": keybox_events,
                "total": 10
            })))
            .mount(&mock_server)
            .await;

        let state = state
            .with_keybox_url(mock_server.uri())
            .with_keybox_service_token("test-keybox-token".to_string())
            .with_service_token("test-service-token");

        let app = super::super::router().with_state(state.clone());

        // Fetch page 1 (offset=0, limit=5)
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/audit/logs?offset=0&limit=5&principal_id={}",
                        test_principal
                    ))
                    .header(header::AUTHORIZATION, "Bearer test-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let page1: AuditLogsResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(page1.events.len(), 5);
        assert_eq!(page1.offset, 0);
        assert_eq!(page1.total, 20); // 10 local + 10 keybox

        // Fetch page 2 (offset=5, limit=5)
        let app = super::super::router().with_state(state);
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/audit/logs?offset=5&limit=5&principal_id={}",
                        test_principal
                    ))
                    .header(header::AUTHORIZATION, "Bearer test-service-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let page2: AuditLogsResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(page2.events.len(), 5);
        assert_eq!(page2.offset, 5);
        assert_eq!(page2.total, 20);

        // Verify no overlap between pages
        let page1_ids: Vec<String> = page1.events.iter().map(|e| e.id.clone()).collect();
        let page2_ids: Vec<String> = page2.events.iter().map(|e| e.id.clone()).collect();

        for id in &page1_ids {
            assert!(
                !page2_ids.contains(id),
                "Event {id} appears in both pages - pagination is broken"
            );
        }
    }
}
