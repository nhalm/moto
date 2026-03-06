//! Integration tests for garage repository.
//!
//! These tests hit real `PostgreSQL`. Run with:
//!     cargo test -p moto-club-db --features integration
//!
//! Requires the test database to be running:
//!     make test-db-up && make test-db-migrate

#[cfg(feature = "integration")]
mod integration_tests {
    use crate::{DbError, GarageStatus, TerminationReason, garage_repo, garage_repo::CreateGarage};
    use moto_test_utils::{test_pool, unique_garage_name, unique_owner};
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

    #[tokio::test]
    async fn create_and_get_by_id() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let id = input.id;

        let created = garage_repo::create(&pool, input).await.unwrap();
        assert_eq!(created.id, id);
        assert_eq!(created.status, GarageStatus::Pending);

        let fetched = garage_repo::get_by_id(&pool, id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, created.name);
        assert_eq!(fetched.owner, created.owner);
    }

    #[tokio::test]
    async fn create_and_get_by_name() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let name = input.name.clone();

        let created = garage_repo::create(&pool, input).await.unwrap();

        let fetched = garage_repo::get_by_name(&pool, &name).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, name);
    }

    #[tokio::test]
    async fn get_by_id_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = garage_repo::get_by_id(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "garage",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn get_by_name_not_found() {
        let pool = test_pool().await;

        let result = garage_repo::get_by_name(&pool, "nonexistent-garage-name").await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "garage",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn create_duplicate_name_fails() {
        let pool = test_pool().await;
        let input1 = create_garage_input();
        let name = input1.name.clone();

        garage_repo::create(&pool, input1).await.unwrap();

        // Try to create another garage with the same name
        let input2 = CreateGarage {
            id: Uuid::now_v7(),
            name,
            owner: unique_owner(),
            branch: "feature".to_string(),
            image: "ghcr.io/test/moto-dev:latest".to_string(),
            ttl_seconds: 7200,
            namespace: format!("moto-garage-{}", Uuid::now_v7()),
            pod_name: "dev-container".to_string(),
        };

        let result = garage_repo::create(&pool, input2).await;
        assert!(matches!(
            result,
            Err(DbError::AlreadyExists {
                entity: "garage",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn list_by_owner() {
        let pool = test_pool().await;
        let owner = unique_owner();

        // Create two garages for this owner
        let input1 = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let input2 = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };

        garage_repo::create(&pool, input1).await.unwrap();
        garage_repo::create(&pool, input2).await.unwrap();

        let garages = garage_repo::list_by_owner(&pool, &owner, false)
            .await
            .unwrap();
        assert_eq!(garages.len(), 2);
        assert!(garages.iter().all(|g| g.owner == owner));
    }

    #[tokio::test]
    async fn list_by_owner_excludes_terminated() {
        let pool = test_pool().await;
        let owner = unique_owner();

        // Create two garages
        let input1 = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let input2 = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };

        let g1 = garage_repo::create(&pool, input1).await.unwrap();
        garage_repo::create(&pool, input2).await.unwrap();

        // Terminate one
        garage_repo::terminate(&pool, g1.id, TerminationReason::UserClosed)
            .await
            .unwrap();

        // List without terminated
        let garages = garage_repo::list_by_owner(&pool, &owner, false)
            .await
            .unwrap();
        assert_eq!(garages.len(), 1);

        // List with terminated
        let garages = garage_repo::list_by_owner(&pool, &owner, true)
            .await
            .unwrap();
        assert_eq!(garages.len(), 2);
    }

    #[tokio::test]
    async fn update_status() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let id = input.id;

        garage_repo::create(&pool, input).await.unwrap();

        let updated = garage_repo::update_status(&pool, id, GarageStatus::Initializing)
            .await
            .unwrap();
        assert_eq!(updated.status, GarageStatus::Initializing);

        let updated = garage_repo::update_status(&pool, id, GarageStatus::Ready)
            .await
            .unwrap();
        assert_eq!(updated.status, GarageStatus::Ready);
    }

    #[tokio::test]
    async fn update_status_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = garage_repo::update_status(&pool, nonexistent_id, GarageStatus::Ready).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "garage",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn terminate_garage() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let id = input.id;

        garage_repo::create(&pool, input).await.unwrap();

        let terminated = garage_repo::terminate(&pool, id, TerminationReason::TtlExpired)
            .await
            .unwrap();
        assert_eq!(terminated.status, GarageStatus::Terminated);
        assert_eq!(
            terminated.termination_reason,
            Some(TerminationReason::TtlExpired)
        );
        assert!(terminated.terminated_at.is_some());
    }

    #[tokio::test]
    async fn extend_ttl() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let id = input.id;

        let created = garage_repo::create(&pool, input).await.unwrap();
        let original_expires_at = created.expires_at;
        let original_ttl = created.ttl_seconds;

        let extended = garage_repo::extend_ttl(&pool, id, 3600).await.unwrap();
        assert_eq!(extended.ttl_seconds, original_ttl + 3600);
        assert!(extended.expires_at > original_expires_at);
    }

    #[tokio::test]
    async fn is_name_available() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let name = input.name.clone();

        // Before creation, name should be available
        assert!(garage_repo::is_name_available(&pool, &name).await.unwrap());

        garage_repo::create(&pool, input).await.unwrap();

        // After creation, name should not be available
        assert!(!garage_repo::is_name_available(&pool, &name).await.unwrap());
    }

    #[tokio::test]
    async fn list_by_status() {
        let pool = test_pool().await;
        let owner = unique_owner();

        // Create a garage and transition it to Ready
        let input = CreateGarage {
            owner: owner.clone(),
            ..create_garage_input()
        };
        let g = garage_repo::create(&pool, input).await.unwrap();
        garage_repo::update_status(&pool, g.id, GarageStatus::Ready)
            .await
            .unwrap();

        // List Ready garages - should include our garage
        let ready_garages = garage_repo::list_by_status(&pool, GarageStatus::Ready)
            .await
            .unwrap();
        assert!(ready_garages.iter().any(|g| g.owner == owner));
    }

    #[tokio::test]
    async fn delete_garage() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let id = input.id;

        garage_repo::create(&pool, input).await.unwrap();

        // Delete should succeed
        garage_repo::delete(&pool, id).await.unwrap();

        // Get should fail
        let result = garage_repo::get_by_id(&pool, id).await;
        assert!(matches!(result, Err(DbError::NotFound { .. })));
    }

    #[tokio::test]
    async fn delete_not_found() {
        let pool = test_pool().await;
        let nonexistent_id = Uuid::now_v7();

        let result = garage_repo::delete(&pool, nonexistent_id).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "garage",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn list_expired_includes_all_non_terminated_states() {
        let pool = test_pool().await;
        let owner = unique_owner();

        // Create garages with 1-second TTL so they expire immediately
        let make_input = || CreateGarage {
            owner: owner.clone(),
            ttl_seconds: 1,
            ..create_garage_input()
        };

        // Create one garage per non-terminated state
        let pending = garage_repo::create(&pool, make_input()).await.unwrap();

        let initializing = garage_repo::create(&pool, make_input()).await.unwrap();
        garage_repo::update_status(&pool, initializing.id, GarageStatus::Initializing)
            .await
            .unwrap();

        let ready = garage_repo::create(&pool, make_input()).await.unwrap();
        garage_repo::update_status(&pool, ready.id, GarageStatus::Ready)
            .await
            .unwrap();

        let failed = garage_repo::create(&pool, make_input()).await.unwrap();
        garage_repo::update_status(&pool, failed.id, GarageStatus::Failed)
            .await
            .unwrap();

        // Also create a terminated garage — should NOT appear in list_expired
        let terminated = garage_repo::create(&pool, make_input()).await.unwrap();
        garage_repo::terminate(&pool, terminated.id, TerminationReason::UserClosed)
            .await
            .unwrap();

        // Wait for TTL to expire
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let expired = garage_repo::list_expired(&pool).await.unwrap();
        let expired_ids: std::collections::HashSet<Uuid> = expired.iter().map(|g| g.id).collect();

        // All non-terminated garages should be in the expired list
        assert!(
            expired_ids.contains(&pending.id),
            "Pending garage should be in expired list"
        );
        assert!(
            expired_ids.contains(&initializing.id),
            "Initializing garage should be in expired list"
        );
        assert!(
            expired_ids.contains(&ready.id),
            "Ready garage should be in expired list"
        );
        assert!(
            expired_ids.contains(&failed.id),
            "Failed garage should be in expired list"
        );

        // Terminated garage should NOT be in the expired list
        assert!(
            !expired_ids.contains(&terminated.id),
            "Terminated garage should NOT be in expired list"
        );
    }

    #[tokio::test]
    async fn terminate_already_terminated_returns_not_found() {
        let pool = test_pool().await;
        let input = create_garage_input();
        let id = input.id;

        garage_repo::create(&pool, input).await.unwrap();

        // First terminate succeeds
        garage_repo::terminate(&pool, id, TerminationReason::UserClosed)
            .await
            .unwrap();

        // Second terminate returns NotFound (already terminated)
        let result = garage_repo::terminate(&pool, id, TerminationReason::TtlExpired).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "garage",
                ..
            })
        ));
    }
}
