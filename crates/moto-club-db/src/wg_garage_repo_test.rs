//! Integration tests for `WireGuard` garage repository.
//!
//! These tests hit real `PostgreSQL`. Run with:
//!     cargo test -p moto-club-db --features integration
//!
//! Requires the test database to be running:
//!     make test-db-up && make test-db-migrate

#[cfg(feature = "integration")]
mod integration_tests {
    use crate::{
        DbError, garage_repo, garage_repo::CreateGarage, wg_garage_repo,
        wg_garage_repo::RegisterWgGarage,
    };
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

    /// Helper: creates a garage and returns its ID.
    async fn setup_garage(pool: &crate::DbPool) -> Uuid {
        let input = create_garage_input();
        let garage = garage_repo::create(pool, input).await.unwrap();
        garage.id
    }

    fn register_input(garage_id: Uuid) -> RegisterWgGarage {
        RegisterWgGarage {
            garage_id,
            public_key: fake_wg_pubkey(),
            endpoints: vec!["10.42.0.5:51820".to_string()],
        }
    }

    // --- register ---

    #[tokio::test]
    async fn register_creates_wg_garage() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        let input = register_input(garage_id);
        let pubkey = input.public_key.clone();

        let wg_garage = wg_garage_repo::register(&pool, input).await.unwrap();
        assert_eq!(wg_garage.garage_id, garage_id);
        assert_eq!(wg_garage.public_key, pubkey);
        assert_eq!(wg_garage.endpoints, vec!["10.42.0.5:51820"]);
        assert!(wg_garage.assigned_ip.starts_with("fd00:moto:1::"));
        assert_eq!(wg_garage.peer_version, 0);
    }

    #[tokio::test]
    async fn register_upserts_on_conflict() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        let input1 = register_input(garage_id);
        let first = wg_garage_repo::register(&pool, input1).await.unwrap();

        // Re-register with a different public key and endpoints
        let new_pubkey = fake_wg_pubkey();
        let input2 = RegisterWgGarage {
            garage_id,
            public_key: new_pubkey.clone(),
            endpoints: vec!["10.42.0.10:51820".to_string()],
        };
        let second = wg_garage_repo::register(&pool, input2).await.unwrap();

        assert_eq!(second.garage_id, garage_id);
        assert_eq!(second.public_key, new_pubkey);
        assert_eq!(second.endpoints, vec!["10.42.0.10:51820"]);
        // Assigned IP is deterministic from garage_id, stays the same
        assert_eq!(second.assigned_ip, first.assigned_ip);
        // peer_version is preserved on upsert
        assert_eq!(second.peer_version, first.peer_version);
    }

    #[tokio::test]
    async fn register_fails_for_nonexistent_garage() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let input = register_input(nonexistent_id);
        let result = wg_garage_repo::register(&pool, input).await;
        assert!(result.is_err());
    }

    // --- get_by_garage_id ---

    #[tokio::test]
    async fn get_by_garage_id_returns_registered() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        let input = register_input(garage_id);
        let pubkey = input.public_key.clone();
        wg_garage_repo::register(&pool, input).await.unwrap();

        let wg_garage = wg_garage_repo::get_by_garage_id(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(wg_garage.garage_id, garage_id);
        assert_eq!(wg_garage.public_key, pubkey);
    }

    #[tokio::test]
    async fn get_by_garage_id_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_garage_repo::get_by_garage_id(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "wg_garage",
                ..
            })
        ));
    }

    // --- exists ---

    #[tokio::test]
    async fn exists_returns_true_when_registered() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        wg_garage_repo::register(&pool, register_input(garage_id))
            .await
            .unwrap();

        assert!(wg_garage_repo::exists(&pool, garage_id).await.unwrap());
    }

    #[tokio::test]
    async fn exists_returns_false_when_not_registered() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        assert!(!wg_garage_repo::exists(&pool, garage_id).await.unwrap());
    }

    #[tokio::test]
    async fn exists_returns_false_for_nonexistent_garage() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        assert!(!wg_garage_repo::exists(&pool, nonexistent_id).await.unwrap());
    }

    // --- update_endpoints ---

    #[tokio::test]
    async fn update_endpoints_changes_endpoints() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        wg_garage_repo::register(&pool, register_input(garage_id))
            .await
            .unwrap();

        let new_endpoints = vec![
            "10.42.0.20:51820".to_string(),
            "10.42.0.21:51820".to_string(),
        ];
        let updated = wg_garage_repo::update_endpoints(&pool, garage_id, &new_endpoints)
            .await
            .unwrap();
        assert_eq!(updated.endpoints, new_endpoints);
        assert_eq!(updated.garage_id, garage_id);
    }

    #[tokio::test]
    async fn update_endpoints_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result =
            wg_garage_repo::update_endpoints(&pool, nonexistent_id, &["10.0.0.1:51820".into()])
                .await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "wg_garage",
                ..
            })
        ));
    }

    // --- increment_peer_version ---

    #[tokio::test]
    async fn increment_peer_version_increments() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        wg_garage_repo::register(&pool, register_input(garage_id))
            .await
            .unwrap();

        let v1 = wg_garage_repo::increment_peer_version(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(v1, 1);

        let v2 = wg_garage_repo::increment_peer_version(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(v2, 2);

        let v3 = wg_garage_repo::increment_peer_version(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(v3, 3);
    }

    #[tokio::test]
    async fn increment_peer_version_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_garage_repo::increment_peer_version(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "wg_garage",
                ..
            })
        ));
    }

    // --- get_peer_version ---

    #[tokio::test]
    async fn get_peer_version_returns_initial_zero() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        wg_garage_repo::register(&pool, register_input(garage_id))
            .await
            .unwrap();

        let version = wg_garage_repo::get_peer_version(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(version, 0);
    }

    #[tokio::test]
    async fn get_peer_version_reflects_increments() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        wg_garage_repo::register(&pool, register_input(garage_id))
            .await
            .unwrap();

        wg_garage_repo::increment_peer_version(&pool, garage_id)
            .await
            .unwrap();
        wg_garage_repo::increment_peer_version(&pool, garage_id)
            .await
            .unwrap();

        let version = wg_garage_repo::get_peer_version(&pool, garage_id)
            .await
            .unwrap();
        assert_eq!(version, 2);
    }

    #[tokio::test]
    async fn get_peer_version_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_garage_repo::get_peer_version(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "wg_garage",
                ..
            })
        ));
    }

    // --- delete ---

    #[tokio::test]
    async fn delete_removes_registration() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        wg_garage_repo::register(&pool, register_input(garage_id))
            .await
            .unwrap();

        wg_garage_repo::delete(&pool, garage_id).await.unwrap();

        assert!(!wg_garage_repo::exists(&pool, garage_id).await.unwrap());
    }

    #[tokio::test]
    async fn delete_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = wg_garage_repo::delete(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "wg_garage",
                ..
            })
        ));
    }

    // --- deterministic IP ---

    #[tokio::test]
    async fn register_assigns_deterministic_ip() {
        let pool = test_pool().await;
        let garage_id = setup_garage(&pool).await;

        let input = register_input(garage_id);
        let first = wg_garage_repo::register(&pool, input).await.unwrap();

        // Delete and re-register - should get the same IP
        wg_garage_repo::delete(&pool, garage_id).await.unwrap();

        let input2 = register_input(garage_id);
        let second = wg_garage_repo::register(&pool, input2).await.unwrap();

        assert_eq!(first.assigned_ip, second.assigned_ip);
    }
}
