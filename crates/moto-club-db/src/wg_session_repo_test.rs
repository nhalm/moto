//! Integration tests for `WireGuard` session repository.
//!
//! These tests hit real `PostgreSQL`. Run with:
//!     cargo test -p moto-club-db --features integration
//!
//! Requires the test database to be running:
//!     make test-db-up && make test-db-migrate

#[cfg(feature = "integration")]
mod integration_tests {
    use crate::{
        DbError, garage_repo,
        garage_repo::CreateGarage,
        wg_device_repo,
        wg_device_repo::CreateWgDevice,
        wg_session_repo,
        wg_session_repo::{CreateWgSession, ListSessionsFilter},
    };
    use chrono::{Duration, Utc};
    use moto_test_utils::{fake_wg_pubkey, test_pool, unique_garage_name, unique_owner};
    use uuid::Uuid;

    fn create_garage_input() -> CreateGarage {
        CreateGarage {
            id: Uuid::now_v7(),
            name: unique_garage_name(),
            owner: unique_owner(),
            branch: "main".to_string(),
            image: "ghcr.io/test/moto-dev:latest".to_string(),
            ttl_seconds: 14400,
            namespace: format!("moto-garage-{}", Uuid::now_v7()),
            pod_name: "dev-container".to_string(),
        }
    }

    fn create_device_input(owner: &str) -> CreateWgDevice {
        CreateWgDevice {
            public_key: fake_wg_pubkey(),
            owner: owner.to_string(),
            device_name: Some("test-device".to_string()),
            assigned_ip: format!("fd00:moto:2::{:x}", Uuid::now_v7().as_u128() & 0xFFFF),
        }
    }

    /// Helper: creates a garage and device, returns (garage_id, device_pubkey, owner).
    async fn setup_garage_and_device(pool: &crate::DbPool) -> (Uuid, String, String) {
        let owner = unique_owner();
        let garage_input = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let garage = garage_repo::create(pool, garage_input).await.unwrap();

        let device_input = create_device_input(&owner);
        let pubkey = device_input.public_key.clone();
        wg_device_repo::create(pool, device_input).await.unwrap();

        (garage.id, pubkey, owner)
    }

    #[tokio::test]
    async fn create_and_get_by_id() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        let expires_at = Utc::now() + Duration::hours(4);
        let input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at,
        };

        let created = wg_session_repo::create(&pool, input).await.unwrap();
        assert_eq!(created.device_pubkey, pubkey);
        assert_eq!(created.garage_id, garage_id);
        assert!(created.closed_at.is_none());

        let fetched = wg_session_repo::get_by_id(&pool, created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.device_pubkey, created.device_pubkey);
        assert_eq!(fetched.garage_id, created.garage_id);
    }

    #[tokio::test]
    async fn get_by_id_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_session_repo::get_by_id(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn list_active_by_device_returns_active_sessions() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        // Create an active session (expires in 4 hours)
        let input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input).await.unwrap();

        let active = wg_session_repo::list_active_by_device(&pool, &pubkey)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].device_pubkey, pubkey);
    }

    #[tokio::test]
    async fn list_active_by_device_excludes_expired() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        // Create an expired session (expired 1 hour ago)
        let input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() - Duration::hours(1),
        };
        wg_session_repo::create(&pool, input).await.unwrap();

        let active = wg_session_repo::list_active_by_device(&pool, &pubkey)
            .await
            .unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn list_active_by_device_excludes_closed() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        // Create a session and close it
        let input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let session = wg_session_repo::create(&pool, input).await.unwrap();
        wg_session_repo::close(&pool, session.id).await.unwrap();

        let active = wg_session_repo::list_active_by_device(&pool, &pubkey)
            .await
            .unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn list_active_by_garage_returns_active_sessions() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        let input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input).await.unwrap();

        let active = wg_session_repo::list_active_by_garage(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].garage_id, garage_id);
    }

    #[tokio::test]
    async fn list_active_by_garage_excludes_expired_and_closed() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        // Create an expired session
        let expired_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() - Duration::hours(1),
        };
        wg_session_repo::create(&pool, expired_input).await.unwrap();

        // Create and close a session
        let closed_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let closed_session = wg_session_repo::create(&pool, closed_input).await.unwrap();
        wg_session_repo::close(&pool, closed_session.id)
            .await
            .unwrap();

        let active = wg_session_repo::list_active_by_garage(&pool, garage_id)
            .await
            .unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn list_all_by_device_includes_expired_and_closed() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        // Active session
        let active_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, active_input).await.unwrap();

        // Expired session
        let expired_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() - Duration::hours(1),
        };
        wg_session_repo::create(&pool, expired_input).await.unwrap();

        // Closed session
        let closed_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let closed_session = wg_session_repo::create(&pool, closed_input).await.unwrap();
        wg_session_repo::close(&pool, closed_session.id)
            .await
            .unwrap();

        let all = wg_session_repo::list_all_by_device(&pool, &pubkey)
            .await
            .unwrap();
        assert_eq!(all.len(), 3);
    }

    #[tokio::test]
    async fn list_by_owner_active_only() {
        let pool = test_pool().await;
        let (garage_id, pubkey, owner) = setup_garage_and_device(&pool).await;

        // Active session
        let active_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, active_input).await.unwrap();

        // Expired session
        let expired_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() - Duration::hours(1),
        };
        wg_session_repo::create(&pool, expired_input).await.unwrap();

        let filter = ListSessionsFilter::default();
        let sessions = wg_session_repo::list_by_owner(&pool, &owner, filter)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
    }

    #[tokio::test]
    async fn list_by_owner_include_all() {
        let pool = test_pool().await;
        let (garage_id, pubkey, owner) = setup_garage_and_device(&pool).await;

        // Active session
        let active_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, active_input).await.unwrap();

        // Expired session
        let expired_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() - Duration::hours(1),
        };
        wg_session_repo::create(&pool, expired_input).await.unwrap();

        let filter = ListSessionsFilter {
            garage_id: None,
            include_all: true,
        };
        let sessions = wg_session_repo::list_by_owner(&pool, &owner, filter)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn list_by_owner_filter_by_garage() {
        let pool = test_pool().await;
        let owner = unique_owner();

        // Create two garages
        let garage1_input = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let garage1 = garage_repo::create(&pool, garage1_input).await.unwrap();

        let garage2_input = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let garage2 = garage_repo::create(&pool, garage2_input).await.unwrap();

        // Create a device for this owner
        let device_input = create_device_input(&owner);
        let pubkey = device_input.public_key.clone();
        wg_device_repo::create(&pool, device_input).await.unwrap();

        // Session on garage1
        let input1 = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id: garage1.id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input1).await.unwrap();

        // Session on garage2
        let input2 = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id: garage2.id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input2).await.unwrap();

        // Filter by garage1
        let filter = ListSessionsFilter {
            garage_id: Some(garage1.id),
            include_all: false,
        };
        let sessions = wg_session_repo::list_by_owner(&pool, &owner, filter)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].garage_id, garage1.id);
    }

    #[tokio::test]
    async fn list_by_owner_with_details_returns_enriched_data() {
        let pool = test_pool().await;
        let (garage_id, pubkey, owner) = setup_garage_and_device(&pool).await;

        let input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input).await.unwrap();

        let filter = ListSessionsFilter::default();
        let sessions = wg_session_repo::list_by_owner_with_details(&pool, &owner, filter)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(!sessions[0].garage_name.is_empty());
        assert_eq!(sessions[0].device_pubkey, pubkey);
    }

    #[tokio::test]
    async fn list_by_owner_with_details_include_all() {
        let pool = test_pool().await;
        let (garage_id, pubkey, owner) = setup_garage_and_device(&pool).await;

        // Active session
        let active_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, active_input).await.unwrap();

        // Expired session
        let expired_input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() - Duration::hours(1),
        };
        wg_session_repo::create(&pool, expired_input).await.unwrap();

        let filter = ListSessionsFilter {
            garage_id: None,
            include_all: true,
        };
        let sessions = wg_session_repo::list_by_owner_with_details(&pool, &owner, filter)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn list_by_owner_with_details_filter_by_garage() {
        let pool = test_pool().await;
        let owner = unique_owner();

        // Two garages
        let garage1_input = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let garage1 = garage_repo::create(&pool, garage1_input).await.unwrap();

        let garage2_input = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let garage2 = garage_repo::create(&pool, garage2_input).await.unwrap();

        let device_input = create_device_input(&owner);
        let pubkey = device_input.public_key.clone();
        wg_device_repo::create(&pool, device_input).await.unwrap();

        // Session on each garage
        let input1 = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id: garage1.id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input1).await.unwrap();

        let input2 = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id: garage2.id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input2).await.unwrap();

        let filter = ListSessionsFilter {
            garage_id: Some(garage2.id),
            include_all: false,
        };
        let sessions = wg_session_repo::list_by_owner_with_details(&pool, &owner, filter)
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].garage_id, garage2.id);
    }

    #[tokio::test]
    async fn close_sets_closed_at() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        let input = CreateWgSession {
            device_pubkey: pubkey,
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let session = wg_session_repo::create(&pool, input).await.unwrap();
        assert!(session.closed_at.is_none());

        let closed = wg_session_repo::close(&pool, session.id).await.unwrap();
        assert!(closed.closed_at.is_some());
    }

    #[tokio::test]
    async fn close_is_idempotent() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        let input = CreateWgSession {
            device_pubkey: pubkey,
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let session = wg_session_repo::create(&pool, input).await.unwrap();

        let first_close = wg_session_repo::close(&pool, session.id).await.unwrap();
        let second_close = wg_session_repo::close(&pool, session.id).await.unwrap();

        // closed_at should be preserved from first close (COALESCE)
        assert_eq!(first_close.closed_at, second_close.closed_at);
    }

    #[tokio::test]
    async fn close_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_session_repo::close(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn close_all_for_garage_closes_active_sessions() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        // Create two active sessions
        for _ in 0..2 {
            let input = CreateWgSession {
                device_pubkey: pubkey.clone(),
                garage_id,
                expires_at: Utc::now() + Duration::hours(4),
            };
            wg_session_repo::create(&pool, input).await.unwrap();
        }

        let closed_count = wg_session_repo::close_all_for_garage(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(closed_count, 2);

        // Verify no active sessions remain
        let active = wg_session_repo::list_active_by_garage(&pool, garage_id)
            .await
            .unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn close_all_for_garage_skips_already_closed() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        // Create and close one session
        let input = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let session = wg_session_repo::create(&pool, input).await.unwrap();
        wg_session_repo::close(&pool, session.id).await.unwrap();

        // Create one active session
        let input2 = CreateWgSession {
            device_pubkey: pubkey.clone(),
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        wg_session_repo::create(&pool, input2).await.unwrap();

        // close_all should only close the one active session
        let closed_count = wg_session_repo::close_all_for_garage(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(closed_count, 1);
    }

    #[tokio::test]
    async fn close_all_for_garage_returns_zero_when_none() {
        let pool = test_pool().await;
        let nonexistent_garage = Uuid::now_v7();

        let closed_count = wg_session_repo::close_all_for_garage(&pool, nonexistent_garage)
            .await
            .unwrap();
        assert_eq!(closed_count, 0);
    }

    #[tokio::test]
    async fn verify_ownership_succeeds_for_owner() {
        let pool = test_pool().await;
        let (garage_id, pubkey, owner) = setup_garage_and_device(&pool).await;

        let input = CreateWgSession {
            device_pubkey: pubkey,
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let session = wg_session_repo::create(&pool, input).await.unwrap();

        let verified = wg_session_repo::verify_ownership(&pool, session.id, &owner)
            .await
            .unwrap();
        assert_eq!(verified.id, session.id);
    }

    #[tokio::test]
    async fn verify_ownership_fails_for_different_owner() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        let input = CreateWgSession {
            device_pubkey: pubkey,
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let session = wg_session_repo::create(&pool, input).await.unwrap();

        let other_owner = unique_owner();
        let result = wg_session_repo::verify_ownership(&pool, session.id, &other_owner).await;
        assert!(matches!(
            result,
            Err(DbError::NotOwned {
                entity: "session",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn verify_ownership_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_session_repo::verify_ownership(&pool, nonexistent_id, "any-owner").await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn delete_removes_session() {
        let pool = test_pool().await;
        let (garage_id, pubkey, _owner) = setup_garage_and_device(&pool).await;

        let input = CreateWgSession {
            device_pubkey: pubkey,
            garage_id,
            expires_at: Utc::now() + Duration::hours(4),
        };
        let session = wg_session_repo::create(&pool, input).await.unwrap();

        wg_session_repo::delete(&pool, session.id).await.unwrap();

        let result = wg_session_repo::get_by_id(&pool, session.id).await;
        assert!(matches!(result, Err(DbError::NotFound { .. })));
    }

    #[tokio::test]
    async fn delete_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_session_repo::delete(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "session",
                ..
            })
        ));
    }
}
