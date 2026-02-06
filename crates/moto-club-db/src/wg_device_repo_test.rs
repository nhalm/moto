//! Integration tests for `WireGuard` device repository.
//!
//! These tests hit real `PostgreSQL`. Run with:
//!     cargo test -p moto-club-db --features integration
//!
//! Requires the test database to be running:
//!     make test-db-up && make test-db-migrate

#[cfg(feature = "integration")]
mod integration_tests {
    use crate::{DbError, wg_device_repo, wg_device_repo::CreateWgDevice};
    use moto_test_utils::{fake_wg_pubkey, test_pool, unique_owner};

    fn create_device_input(assigned_ip: &str) -> CreateWgDevice {
        CreateWgDevice {
            public_key: fake_wg_pubkey(),
            owner: unique_owner(),
            device_name: Some("test-device".to_string()),
            assigned_ip: assigned_ip.to_string(),
        }
    }

    #[tokio::test]
    async fn create_and_get_by_public_key() {
        let pool = test_pool().await;
        let input = create_device_input("fd00:moto:2::1");
        let public_key = input.public_key.clone();

        let created = wg_device_repo::create(&pool, input).await.unwrap();
        assert_eq!(created.public_key, public_key);

        let fetched = wg_device_repo::get_by_public_key(&pool, &public_key)
            .await
            .unwrap();
        assert_eq!(fetched.public_key, created.public_key);
        assert_eq!(fetched.owner, created.owner);
        assert_eq!(fetched.device_name, created.device_name);
        assert_eq!(fetched.assigned_ip, created.assigned_ip);
    }

    #[tokio::test]
    async fn get_by_public_key_not_found() {
        let pool = test_pool().await;
        let nonexistent_key = fake_wg_pubkey();

        let result = wg_device_repo::get_by_public_key(&pool, &nonexistent_key).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "device",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn exists_returns_true_for_existing_device() {
        let pool = test_pool().await;
        let input = create_device_input("fd00:moto:2::2");
        let public_key = input.public_key.clone();

        wg_device_repo::create(&pool, input).await.unwrap();

        assert!(wg_device_repo::exists(&pool, &public_key).await.unwrap());
    }

    #[tokio::test]
    async fn exists_returns_false_for_nonexistent_device() {
        let pool = test_pool().await;
        let nonexistent_key = fake_wg_pubkey();

        assert!(
            !wg_device_repo::exists(&pool, &nonexistent_key)
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn create_duplicate_public_key_fails() {
        let pool = test_pool().await;
        let input1 = create_device_input("fd00:moto:2::3");
        let public_key = input1.public_key.clone();

        wg_device_repo::create(&pool, input1).await.unwrap();

        // Try to create another device with the same public key
        let input2 = CreateWgDevice {
            public_key: public_key.clone(),
            owner: unique_owner(),
            device_name: Some("another-device".to_string()),
            assigned_ip: "fd00:moto:2::4".to_string(),
        };

        let result = wg_device_repo::create(&pool, input2).await;
        assert!(matches!(
            result,
            Err(DbError::AlreadyExists {
                entity: "device",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn get_or_create_creates_new_device() {
        let pool = test_pool().await;
        let input = create_device_input("fd00:moto:2::5");
        let public_key = input.public_key.clone();

        let (device, created) = wg_device_repo::get_or_create(&pool, input).await.unwrap();
        assert!(created);
        assert_eq!(device.public_key, public_key);
    }

    #[tokio::test]
    async fn get_or_create_returns_existing_device_same_owner() {
        let pool = test_pool().await;
        let owner = unique_owner();
        let input1 = CreateWgDevice {
            public_key: fake_wg_pubkey(),
            owner: owner.clone(),
            device_name: Some("device-1".to_string()),
            assigned_ip: "fd00:moto:2::6".to_string(),
        };
        let public_key = input1.public_key.clone();

        // First creation
        let (device1, created1) = wg_device_repo::get_or_create(&pool, input1).await.unwrap();
        assert!(created1);

        // Second call with same key and owner should return existing
        let input2 = CreateWgDevice {
            public_key: public_key.clone(),
            owner: owner.clone(),
            device_name: Some("device-updated".to_string()),
            assigned_ip: "fd00:moto:2::7".to_string(),
        };

        let (device2, created2) = wg_device_repo::get_or_create(&pool, input2).await.unwrap();
        assert!(!created2);
        assert_eq!(device2.public_key, device1.public_key);
        // Original values should be preserved
        assert_eq!(device2.device_name, device1.device_name);
        assert_eq!(device2.assigned_ip, device1.assigned_ip);
    }

    #[tokio::test]
    async fn get_or_create_fails_for_different_owner() {
        let pool = test_pool().await;
        let input1 = create_device_input("fd00:moto:2::8");
        let public_key = input1.public_key.clone();

        wg_device_repo::create(&pool, input1).await.unwrap();

        // Try to get_or_create with same key but different owner
        let input2 = CreateWgDevice {
            public_key,
            owner: unique_owner(), // Different owner
            device_name: Some("attacker-device".to_string()),
            assigned_ip: "fd00:moto:2::9".to_string(),
        };

        let result = wg_device_repo::get_or_create(&pool, input2).await;
        assert!(matches!(
            result,
            Err(DbError::NotOwned {
                entity: "device",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn list_by_owner_returns_all_devices_for_owner() {
        let pool = test_pool().await;
        let owner = unique_owner();

        // Create two devices for this owner
        let input1 = CreateWgDevice {
            public_key: fake_wg_pubkey(),
            owner: owner.clone(),
            device_name: Some("device-1".to_string()),
            assigned_ip: "fd00:moto:2::a".to_string(),
        };
        let input2 = CreateWgDevice {
            public_key: fake_wg_pubkey(),
            owner: owner.clone(),
            device_name: Some("device-2".to_string()),
            assigned_ip: "fd00:moto:2::b".to_string(),
        };

        wg_device_repo::create(&pool, input1).await.unwrap();
        wg_device_repo::create(&pool, input2).await.unwrap();

        let devices = wg_device_repo::list_by_owner(&pool, &owner).await.unwrap();
        assert_eq!(devices.len(), 2);
        assert!(devices.iter().all(|d| d.owner == owner));
    }

    #[tokio::test]
    async fn list_by_owner_returns_empty_for_no_devices() {
        let pool = test_pool().await;
        let owner = unique_owner();

        let devices = wg_device_repo::list_by_owner(&pool, &owner).await.unwrap();
        assert!(devices.is_empty());
    }

    #[tokio::test]
    async fn delete_removes_device() {
        let pool = test_pool().await;
        let input = create_device_input("fd00:moto:2::c");
        let public_key = input.public_key.clone();

        wg_device_repo::create(&pool, input).await.unwrap();

        // Delete should succeed
        wg_device_repo::delete(&pool, &public_key).await.unwrap();

        // Get should fail
        let result = wg_device_repo::get_by_public_key(&pool, &public_key).await;
        assert!(matches!(result, Err(DbError::NotFound { .. })));
    }

    #[tokio::test]
    async fn delete_not_found() {
        let pool = test_pool().await;
        let nonexistent_key = fake_wg_pubkey();

        let result = wg_device_repo::delete(&pool, &nonexistent_key).await;
        assert!(matches!(
            result,
            Err(DbError::NotFound {
                entity: "device",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn create_device_without_name() {
        let pool = test_pool().await;
        let input = CreateWgDevice {
            public_key: fake_wg_pubkey(),
            owner: unique_owner(),
            device_name: None,
            assigned_ip: "fd00:moto:2::d".to_string(),
        };
        let public_key = input.public_key.clone();

        let created = wg_device_repo::create(&pool, input).await.unwrap();
        assert_eq!(created.public_key, public_key);
        assert_eq!(created.device_name, None);
    }
}
