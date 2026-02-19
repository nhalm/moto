//! Tests for `WireGuard` coordination REST endpoints.
//!
//! Per AGENTS.md test organization convention, tests for `wg.rs` are in this separate file.
//!
//! These tests use in-memory stores from `moto-club-wg` for unit testing.
//! Integration tests that require full `PostgreSQL` storage belong in `tests/`.

use super::*;
use moto_wgtunnel_types::WgPrivateKey;

// Helper to generate a valid public key
fn test_public_key() -> WgPublicKey {
    WgPrivateKey::generate().public_key()
}

#[test]
fn register_device_request_deserialize() {
    let key = test_public_key();
    let json = format!(
        r#"{{
        "public_key": "{}",
        "device_name": "my-laptop"
    }}"#,
        key.to_base64()
    );
    let req: RegisterDeviceRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.device_name, Some("my-laptop".to_string()));
}

#[test]
fn register_device_request_optional_name() {
    let key = test_public_key();
    let json = format!(r#"{{"public_key": "{}"}}"#, key.to_base64());
    let req: RegisterDeviceRequest = serde_json::from_str(&json).unwrap();
    assert!(req.device_name.is_none());
}

#[test]
fn create_session_request_deserialize() {
    let garage_id = Uuid::now_v7();
    let device_key = test_public_key();
    let json = format!(
        r#"{{
            "garage_id": "{}",
            "device_pubkey": "{}",
            "ttl_seconds": 3600
        }}"#,
        garage_id,
        device_key.to_base64()
    );
    let req: CreateSessionRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.garage_id, garage_id);
    assert_eq!(req.device_pubkey, device_key);
    assert_eq!(req.ttl_seconds, Some(3600));
}

#[test]
fn create_session_request_optional_ttl() {
    let garage_id = Uuid::now_v7();
    let device_key = test_public_key();
    let json = format!(
        r#"{{
            "garage_id": "{}",
            "device_pubkey": "{}"
        }}"#,
        garage_id,
        device_key.to_base64()
    );
    let req: CreateSessionRequest = serde_json::from_str(&json).unwrap();
    assert!(req.ttl_seconds.is_none());
}

#[test]
fn register_garage_request_deserialize() {
    let key = test_public_key();
    let json = format!(
        r#"{{
        "garage_id": "feature-foo",
        "public_key": "{}",
        "endpoints": ["10.0.0.1:51820"]
    }}"#,
        key.to_base64()
    );
    let req: RegisterGarageRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(req.garage_id, "feature-foo");
    assert_eq!(req.endpoints.len(), 1);
}

#[test]
fn register_garage_request_no_endpoints() {
    let key = test_public_key();
    let json = format!(
        r#"{{
        "garage_id": "feature-foo",
        "public_key": "{}"
    }}"#,
        key.to_base64()
    );
    let req: RegisterGarageRequest = serde_json::from_str(&json).unwrap();
    assert!(req.endpoints.is_empty());
}

#[test]
fn device_response_serialize() {
    let key = test_public_key();
    let response = DeviceResponse {
        public_key: key,
        overlay_ip: OverlayIp::client(1),
        device_name: Some("test".to_string()),
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("public_key"));
    assert!(json.contains("assigned_ip")); // Note: renamed from overlay_ip
}

#[test]
fn list_sessions_response_serialize() {
    let response = ListSessionsResponse { sessions: vec![] };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("sessions"));
    assert!(json.contains("[]"));
}

#[test]
fn list_sessions_query_defaults() {
    let query: ListSessionsQuery = serde_json::from_str("{}").unwrap();
    assert!(query.garage_id.is_none());
    assert!(!query.all);
}

#[test]
fn list_sessions_query_with_garage_id() {
    let json = r#"{"garage_id": "01234567-89ab-cdef-0123-456789abcdef"}"#;
    let query: ListSessionsQuery = serde_json::from_str(json).unwrap();
    assert!(query.garage_id.is_some());
    assert_eq!(
        query.garage_id.unwrap().to_string(),
        "01234567-89ab-cdef-0123-456789abcdef"
    );
    assert!(!query.all);
}

#[test]
fn list_sessions_query_with_all() {
    let json = r#"{"all": true}"#;
    let query: ListSessionsQuery = serde_json::from_str(json).unwrap();
    assert!(query.garage_id.is_none());
    assert!(query.all);
}

#[test]
fn list_sessions_query_with_both() {
    let json = r#"{"garage_id": "01234567-89ab-cdef-0123-456789abcdef", "all": true}"#;
    let query: ListSessionsQuery = serde_json::from_str(json).unwrap();
    assert!(query.garage_id.is_some());
    assert!(query.all);
}

#[test]
fn peer_list_response_serialize() {
    let response = PeerListResponse {
        peers: vec![],
        version: 42,
    };
    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("peers"));
    assert!(json.contains(r#""version":42"#));
}

#[test]
fn derp_map_response_serialize() {
    use moto_wgtunnel_types::derp::{DerpNode, DerpRegion};

    let derp_map = DerpMap::new().with_region(
        DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("derp.example.com")),
    );
    let response = DerpMapResponse {
        derp_map,
        version: 5,
    };
    let json = serde_json::to_string(&response).unwrap();

    // Should have regions (flattened from DerpMap)
    assert!(json.contains("regions"));
    // Should have version
    assert!(json.contains(r#""version":5"#));
    // Should have the region data
    assert!(json.contains("primary"));
    assert!(json.contains("derp.example.com"));
}

#[test]
fn derp_map_response_matches_spec_format() {
    // Verify response format matches the spec:
    // {
    //   "regions": { "1": { "name": "primary", "nodes": [...] } },
    //   "version": 5
    // }
    use moto_wgtunnel_types::derp::{DerpNode, DerpRegion};

    let derp_map = DerpMap::new().with_region(
        DerpRegion::new(1, "primary").with_node(DerpNode::new("derp.example.com", 443, 3478)),
    );
    let response = DerpMapResponse {
        derp_map,
        version: 1,
    };

    let json = serde_json::to_string_pretty(&response).unwrap();
    // Parse back to verify structure
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert!(parsed["regions"].is_object());
    assert!(parsed["regions"]["1"].is_object());
    assert_eq!(parsed["regions"]["1"]["name"], "primary");
    assert!(parsed["regions"]["1"]["nodes"].is_array());
    assert_eq!(parsed["version"], 1);
}

// Handler tests require PostgreSQL. Run with: cargo test --features integration
//
// Per spec v1.8 changelog: "Convert ignored integration tests to use moto-test-utils"
#[cfg(all(test, feature = "integration"))]
mod handler_tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{Request, header};
    use moto_club_wg::{Ipam, PeerBroadcaster, PeerRegistry, SessionManager};
    use moto_test_utils::{test_pool, unique_owner};
    use moto_wgtunnel_types::derp::{DerpNode, DerpRegion};
    use tower::ServiceExt;

    use crate::{PostgresIpamStore, PostgresPeerStore, PostgresSessionStore};

    async fn create_test_state() -> AppState {
        let db_pool = test_pool().await;

        // Create PostgreSQL-backed stores
        let ipam_store = PostgresIpamStore::new(db_pool.clone());
        let peer_store = PostgresPeerStore::new(db_pool.clone());
        let session_store = PostgresSessionStore::new(db_pool.clone());

        // Create a default DERP map for testing
        let derp_map = DerpMap::new().with_region(
            DerpRegion::new(1, "primary").with_node(DerpNode::with_defaults("derp.moto.dev")),
        );

        let ipam = Ipam::new(ipam_store);
        let peer_registry = Arc::new(PeerRegistry::new(peer_store, ipam));
        let session_manager = Arc::new(SessionManager::new(session_store));
        let peer_broadcaster = Arc::new(PeerBroadcaster::new());

        AppState::new(
            db_pool,
            peer_registry,
            session_manager,
            derp_map,
            peer_broadcaster,
        )
    }

    // Helper to generate unique test owner for isolation
    fn test_owner() -> String {
        unique_owner()
    }

    #[tokio::test]
    async fn register_device_success() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        let owner = test_owner();

        let key = test_public_key();
        let body = serde_json::json!({
            "public_key": key.to_base64(),
            "device_name": "test-laptop"
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/devices")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let device: DeviceResponse = serde_json::from_slice(&body).unwrap();

        assert_eq!(device.device_name, Some("test-laptop".to_string()));
        assert!(device.overlay_ip.is_client());
    }

    #[tokio::test]
    async fn register_device_without_name() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        let owner = test_owner();

        let key = test_public_key();
        let body = serde_json::json!({
            "public_key": key.to_base64()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/devices")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let device: DeviceResponse = serde_json::from_slice(&body).unwrap();

        assert!(device.device_name.is_none());
    }

    #[tokio::test]
    async fn register_device_requires_auth() {
        let state = create_test_state().await;
        let app = router().with_state(state);

        let key = test_public_key();
        let body = serde_json::json!({
            "public_key": key.to_base64()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/devices")
                    .header(header::CONTENT_TYPE, "application/json")
                    // No authorization header
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_device_not_found() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        let owner = test_owner();

        // Use a non-existent public key (base64 is URL-safe except for + and /)
        // We'll percent-encode the key manually for the test
        let nonexistent_key = test_public_key();
        let key_base64 = nonexistent_key.to_base64();
        // URL-encode the base64 string (replace + with %2B, / with %2F, = with %3D)
        let key_encoded = key_base64
            .replace('+', "%2B")
            .replace('/', "%2F")
            .replace('=', "%3D");
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/wg/devices/{key_encoded}"))
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn register_then_get_device() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);
        let owner = test_owner();

        // Register a device
        let key = test_public_key();
        let body = serde_json::json!({
            "public_key": key.to_base64(),
            "device_name": "test-device"
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/devices")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let registered: DeviceResponse = serde_json::from_slice(&body).unwrap();

        // Now get the device - use the peer_registry directly since we need state
        let device = peer_registry.get_device(&registered.public_key).unwrap();
        assert!(device.is_some());
        let device = device.unwrap();
        assert_eq!(device.public_key, registered.public_key);
        assert_eq!(device.overlay_ip, registered.overlay_ip);
    }

    #[tokio::test]
    async fn create_session_device_not_found() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        let owner = test_owner();

        // Use an unregistered device public key
        let device_key = test_public_key();
        let garage_id = Uuid::now_v7();
        let body = serde_json::json!({
            "garage_id": garage_id.to_string(),
            "device_pubkey": device_key.to_base64()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_session_garage_not_found() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);
        let owner = test_owner();

        // Register a device first
        let device_key = test_public_key();
        peer_registry
            .register_device(moto_club_wg::DeviceRegistration {
                public_key: device_key.clone(),
                owner: owner.clone(),
                device_name: Some("test-device".to_string()),
            })
            .await
            .unwrap();

        let nonexistent_garage_id = Uuid::now_v7();
        let body = serde_json::json!({
            "garage_id": nonexistent_garage_id.to_string(),
            "device_pubkey": device_key.to_base64()
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn create_session_success() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);
        let owner = test_owner();

        // Register a device
        let device_key = test_public_key();
        peer_registry
            .register_device(moto_club_wg::DeviceRegistration {
                public_key: device_key.clone(),
                owner: owner.clone(),
                device_name: Some("test-device".to_string()),
            })
            .await
            .unwrap();

        // Register a garage (using UUID as garage_id)
        let garage_id = Uuid::now_v7();
        let garage_key = test_public_key();
        peer_registry
            .register_garage(moto_club_wg::GarageRegistration {
                garage_id: garage_id.to_string(),
                public_key: garage_key,
                endpoints: vec!["10.0.0.1:51820".parse().unwrap()],
            })
            .await
            .unwrap();

        let body = serde_json::json!({
            "garage_id": garage_id.to_string(),
            "device_pubkey": device_key.to_base64(),
            "ttl_seconds": 3600
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: SessionResponse = serde_json::from_slice(&body).unwrap();

        assert!(session.session_id.starts_with("sess_"));
        assert!(session.client_ip.is_client());
        assert!(session.garage.overlay_ip.is_garage());
    }

    #[tokio::test]
    async fn close_session_not_found() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        let owner = test_owner();

        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/v1/wg/sessions/sess_nonexistent")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn register_garage_success() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        // Use unique garage_id for test isolation
        let garage_id = format!("test-garage-{}", Uuid::now_v7());

        let key = test_public_key();
        let body = serde_json::json!({
            "garage_id": garage_id,
            "public_key": key.to_base64(),
            "endpoints": ["10.0.0.1:51820"]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/garages")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                    .body(Body::from(serde_json::to_string(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let garage: GarageWgResponse = serde_json::from_slice(&body).unwrap();

        // Assigned IP should be in the garage subnet
        assert!(garage.assigned_ip.is_garage());
        // DERP map should have at least one region
        assert!(!garage.derp_map.regions().is_empty());
    }

    #[tokio::test]
    async fn get_garage_wg_registration_not_found() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        // Use unique nonexistent garage_id
        let nonexistent_garage_id = format!("nonexistent-garage-{}", Uuid::now_v7());

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/wg/garages/{nonexistent_garage_id}"))
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_garage_wg_registration_success() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);

        // Register a garage first with unique id
        let garage_id = format!("test-garage-for-get-{}", Uuid::now_v7());
        let key = test_public_key();
        peer_registry
            .register_garage(moto_club_wg::GarageRegistration {
                garage_id: garage_id.clone(),
                public_key: key.clone(),
                endpoints: vec!["10.0.0.1:51820".parse().unwrap()],
            })
            .await
            .unwrap();

        // Now get the registration
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/wg/garages/{garage_id}"))
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let registration: GarageWgRegistrationResponse = serde_json::from_slice(&body).unwrap();

        // Verify response fields
        assert_eq!(registration.garage_id, garage_id);
        assert_eq!(registration.public_key, key);
        assert!(registration.assigned_ip.is_garage());
        assert_eq!(registration.endpoints.len(), 1);
        assert!(registration.peer_version >= 0); // Version from database
        assert!(!registration.derp_map.regions().is_empty());
    }

    #[tokio::test]
    async fn create_and_close_session() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);
        let owner = test_owner();

        // Register device and garage
        let device_key = test_public_key();
        peer_registry
            .register_device(moto_club_wg::DeviceRegistration {
                public_key: device_key.clone(),
                owner: owner.clone(),
                device_name: None,
            })
            .await
            .unwrap();

        let garage_id = Uuid::now_v7();
        let garage_key = test_public_key();
        peer_registry
            .register_garage(moto_club_wg::GarageRegistration {
                garage_id: garage_id.to_string(),
                public_key: garage_key,
                endpoints: vec![],
            })
            .await
            .unwrap();

        // Create session
        let create_body = serde_json::json!({
            "garage_id": garage_id.to_string(),
            "device_pubkey": device_key.to_base64()
        });

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/wg/sessions")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::from(serde_json::to_string(&create_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let session: SessionResponse = serde_json::from_slice(&body).unwrap();

        // Close session
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/v1/wg/sessions/{}", session.session_id))
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn get_derp_map_success() {
        let state = create_test_state().await;
        let app = router().with_state(state);
        let owner = test_owner();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/wg/derp-map")
                    .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        // Parse as generic JSON since DerpMapResponse doesn't implement Deserialize
        // (due to serde(flatten) with HashMap<u16, _> not round-tripping correctly)
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Verify version field
        assert_eq!(result["version"], 1);

        // Verify regions field exists and has expected structure
        assert!(result["regions"].is_object());
        let regions = result["regions"].as_object().unwrap();
        assert!(!regions.is_empty());

        // Default test state includes one region with derp.moto.dev
        assert!(regions.contains_key("1"));
        let primary_region = &regions["1"];
        assert_eq!(primary_region["name"], "primary");
        assert!(primary_region["nodes"].is_array());
        assert_eq!(primary_region["nodes"][0]["host"], "derp.moto.dev");
    }

    #[tokio::test]
    async fn get_derp_map_requires_auth() {
        let state = create_test_state().await;
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/wg/derp-map")
                    // No authorization header
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_derp_map_accepts_k8s_token() {
        // K8s service account tokens should also be accepted
        let state = create_test_state().await;
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/wg/derp-map")
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_garage_peers_returns_version() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);

        // First register a garage so we have something to query
        let garage_id = format!("test-garage-peers-{}", Uuid::now_v7());
        let key = test_public_key();
        peer_registry
            .register_garage(moto_club_wg::GarageRegistration {
                garage_id: garage_id.clone(),
                public_key: key,
                endpoints: vec![],
            })
            .await
            .unwrap();

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/wg/garages/{garage_id}/peers"))
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Should have peers array and version field
        assert!(result["peers"].is_array());
        assert!(result["version"].is_number());
    }

    #[tokio::test]
    async fn get_garage_peers_conditional_304() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);

        // First register a garage
        let garage_id = format!("test-garage-304-{}", Uuid::now_v7());
        let key = test_public_key();
        peer_registry
            .register_garage(moto_club_wg::GarageRegistration {
                garage_id: garage_id.clone(),
                public_key: key,
                endpoints: vec![],
            })
            .await
            .unwrap();

        // First request without version param
        let response1 = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/wg/garages/{garage_id}/peers"))
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response1.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response1.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let version = result["version"].as_i64().unwrap();

        // Second request with matching version should return 304
        let response2 = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!(
                        "/api/v1/wg/garages/{garage_id}/peers?version={version}"
                    ))
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn get_garage_peers_conditional_200_on_version_mismatch() {
        let state = create_test_state().await;
        let peer_registry = state.peer_registry.clone();
        let app = router().with_state(state);

        // First register a garage
        let garage_id = format!("test-garage-200-{}", Uuid::now_v7());
        let key = test_public_key();
        peer_registry
            .register_garage(moto_club_wg::GarageRegistration {
                garage_id: garage_id.clone(),
                public_key: key,
                endpoints: vec![],
            })
            .await
            .unwrap();

        // Request with a different version should return 200
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/v1/wg/garages/{garage_id}/peers?version=999"))
                    .header(header::AUTHORIZATION, "Bearer k8s-service-account")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Should have peers and version
        assert!(result["peers"].is_array());
        assert!(result["version"].is_number());
    }
}
