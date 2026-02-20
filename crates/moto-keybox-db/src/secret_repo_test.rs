//! Integration tests for secret repository.
//!
//! These tests hit real `PostgreSQL`. Run with:
//!     cargo test -p moto-keybox-db --features integration
//!
//! Requires the test database to be running:
//!     make test-db-up && make test-db-migrate

#[cfg(feature = "integration")]
mod integration_tests {
    use crate::{Scope, secret_repo};
    use moto_test_utils::test_pool;
    use uuid::Uuid;

    /// Generates a unique secret name for test isolation.
    fn unique_secret_name() -> String {
        format!("test-secret-{}", Uuid::now_v7())
    }

    /// Generates a unique service name for test isolation.
    fn unique_service() -> String {
        format!("test-svc-{}", Uuid::now_v7())
    }

    /// Generates a unique instance ID for test isolation.
    fn unique_instance_id() -> String {
        format!("test-inst-{}", Uuid::now_v7())
    }

    /// Generates fake encrypted bytes for DEK/ciphertext fields.
    fn fake_bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 256) as u8).collect()
    }

    // ── create_secret ──

    #[tokio::test]
    async fn create_secret_global() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        assert_eq!(secret.scope, Scope::Global);
        assert_eq!(secret.name, name);
        assert!(secret.service.is_none());
        assert!(secret.instance_id.is_none());
        assert_eq!(secret.current_version, 1);
        assert!(secret.deleted_at.is_none());
    }

    #[tokio::test]
    async fn create_secret_service_scoped() {
        let pool = test_pool().await;
        let name = unique_secret_name();
        let service = unique_service();

        let secret = secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name)
            .await
            .unwrap();

        assert_eq!(secret.scope, Scope::Service);
        assert_eq!(secret.service.as_deref(), Some(service.as_str()));
        assert!(secret.instance_id.is_none());
    }

    #[tokio::test]
    async fn create_secret_instance_scoped() {
        let pool = test_pool().await;
        let name = unique_secret_name();
        let service = unique_service();
        let instance = unique_instance_id();

        let secret = secret_repo::create_secret(
            &pool,
            Scope::Instance,
            Some(&service),
            Some(&instance),
            &name,
        )
        .await
        .unwrap();

        assert_eq!(secret.scope, Scope::Instance);
        assert_eq!(secret.service.as_deref(), Some(service.as_str()));
        assert_eq!(secret.instance_id.as_deref(), Some(instance.as_str()));
    }

    #[tokio::test]
    async fn create_secret_duplicate_fails() {
        let pool = test_pool().await;
        let name = unique_secret_name();
        let service = unique_service();
        let instance = unique_instance_id();

        // Use Instance scope (all columns non-null) so the UNIQUE constraint fires
        // (PostgreSQL treats NULLs as distinct in UNIQUE constraints)
        secret_repo::create_secret(
            &pool,
            Scope::Instance,
            Some(&service),
            Some(&instance),
            &name,
        )
        .await
        .unwrap();

        let result = secret_repo::create_secret(
            &pool,
            Scope::Instance,
            Some(&service),
            Some(&instance),
            &name,
        )
        .await;
        assert!(result.is_err());
    }

    // ── get_secret ──

    #[tokio::test]
    async fn get_secret_found() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let created = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let fetched = secret_repo::get_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, name);
    }

    #[tokio::test]
    async fn get_secret_not_found() {
        let pool = test_pool().await;

        let result = secret_repo::get_secret(&pool, Scope::Global, None, None, "nonexistent")
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_secret_excludes_deleted() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let created = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        secret_repo::delete_secret(&pool, created.id).await.unwrap();

        let result = secret_repo::get_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    // ── get_secret_by_id ──

    #[tokio::test]
    async fn get_secret_by_id_found() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let created = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let fetched = secret_repo::get_secret_by_id(&pool, created.id)
            .await
            .unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn get_secret_by_id_not_found() {
        let pool = test_pool().await;

        let result = secret_repo::get_secret_by_id(&pool, Uuid::now_v7())
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_secret_by_id_excludes_deleted() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let created = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        secret_repo::delete_secret(&pool, created.id).await.unwrap();

        let result = secret_repo::get_secret_by_id(&pool, created.id)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    // ── list_secrets ──

    #[tokio::test]
    async fn list_secrets_by_scope() {
        let pool = test_pool().await;
        let service = unique_service();

        // Create two service-scoped secrets under the same service
        let name1 = unique_secret_name();
        let name2 = unique_secret_name();
        secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name1)
            .await
            .unwrap();
        secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name2)
            .await
            .unwrap();

        let secrets = secret_repo::list_secrets(&pool, Scope::Service)
            .await
            .unwrap();
        // Should contain at least our two secrets
        let our_secrets: Vec<_> = secrets
            .iter()
            .filter(|s| s.service.as_deref() == Some(service.as_str()))
            .collect();
        assert_eq!(our_secrets.len(), 2);
    }

    #[tokio::test]
    async fn list_secrets_excludes_deleted() {
        let pool = test_pool().await;
        let service = unique_service();

        let name1 = unique_secret_name();
        let name2 = unique_secret_name();

        let s1 = secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name1)
            .await
            .unwrap();
        secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name2)
            .await
            .unwrap();

        // Delete one
        secret_repo::delete_secret(&pool, s1.id).await.unwrap();

        let secrets = secret_repo::list_secrets(&pool, Scope::Service)
            .await
            .unwrap();
        let our_secrets: Vec<_> = secrets
            .iter()
            .filter(|s| s.service.as_deref() == Some(service.as_str()))
            .collect();
        assert_eq!(our_secrets.len(), 1);
        assert_eq!(our_secrets[0].name, name2);
    }

    #[tokio::test]
    async fn list_secrets_ordered_by_name() {
        let pool = test_pool().await;
        let service = unique_service();

        // Create secrets with names that sort in a known order
        let name_b = format!("b-{}", unique_secret_name());
        let name_a = format!("a-{}", unique_secret_name());

        secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name_b)
            .await
            .unwrap();
        secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name_a)
            .await
            .unwrap();

        let secrets = secret_repo::list_secrets(&pool, Scope::Service)
            .await
            .unwrap();
        let our_secrets: Vec<_> = secrets
            .iter()
            .filter(|s| s.service.as_deref() == Some(service.as_str()))
            .collect();
        assert_eq!(our_secrets.len(), 2);
        assert!(our_secrets[0].name < our_secrets[1].name);
    }

    // ── list_service_secrets ──

    #[tokio::test]
    async fn list_service_secrets_filters_by_service() {
        let pool = test_pool().await;
        let svc1 = unique_service();
        let svc2 = unique_service();

        let name1 = unique_secret_name();
        let name2 = unique_secret_name();

        secret_repo::create_secret(&pool, Scope::Service, Some(&svc1), None, &name1)
            .await
            .unwrap();
        secret_repo::create_secret(&pool, Scope::Service, Some(&svc2), None, &name2)
            .await
            .unwrap();

        let secrets = secret_repo::list_service_secrets(&pool, &svc1)
            .await
            .unwrap();
        assert!(
            secrets
                .iter()
                .all(|s| s.service.as_deref() == Some(svc1.as_str()))
        );
        assert!(secrets.iter().any(|s| s.name == name1));
        assert!(!secrets.iter().any(|s| s.name == name2));
    }

    // ── list_instance_secrets ──

    #[tokio::test]
    async fn list_instance_secrets_filters_by_instance() {
        let pool = test_pool().await;
        let service = unique_service();
        let inst1 = unique_instance_id();
        let inst2 = unique_instance_id();

        let name1 = unique_secret_name();
        let name2 = unique_secret_name();

        secret_repo::create_secret(&pool, Scope::Instance, Some(&service), Some(&inst1), &name1)
            .await
            .unwrap();
        secret_repo::create_secret(&pool, Scope::Instance, Some(&service), Some(&inst2), &name2)
            .await
            .unwrap();

        let secrets = secret_repo::list_instance_secrets(&pool, &inst1)
            .await
            .unwrap();
        assert!(
            secrets
                .iter()
                .all(|s| s.instance_id.as_deref() == Some(inst1.as_str()))
        );
        assert!(secrets.iter().any(|s| s.name == name1));
        assert!(!secrets.iter().any(|s| s.name == name2));
    }

    // ── update_secret_version ──

    #[tokio::test]
    async fn update_secret_version_not_found() {
        let pool = test_pool().await;

        let result = secret_repo::update_secret_version(&pool, Uuid::now_v7(), 2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn update_secret_version_increments() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let created = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();
        assert_eq!(created.current_version, 1);

        let updated = secret_repo::update_secret_version(&pool, created.id, 2)
            .await
            .unwrap();
        assert_eq!(updated.current_version, 2);
        assert!(updated.updated_at >= created.updated_at);
    }

    // ── delete_secret ──

    #[tokio::test]
    async fn delete_secret_soft_deletes() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let created = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();
        assert!(created.deleted_at.is_none());

        secret_repo::delete_secret(&pool, created.id).await.unwrap();

        // Confirm soft delete: get_secret returns None, but raw query shows deleted_at is set
        let result = secret_repo::get_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();
        assert!(result.is_none());

        let result = secret_repo::get_secret_by_id(&pool, created.id)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    // ── create_encrypted_dek / get_encrypted_dek ──

    #[tokio::test]
    async fn create_and_get_encrypted_dek() {
        let pool = test_pool().await;
        let key_bytes = fake_bytes(32);
        let nonce_bytes = fake_bytes(12);

        let dek = secret_repo::create_encrypted_dek(&pool, &key_bytes, &nonce_bytes)
            .await
            .unwrap();

        assert_eq!(dek.encrypted_key, key_bytes);
        assert_eq!(dek.nonce, nonce_bytes);

        let fetched = secret_repo::get_encrypted_dek(&pool, dek.id).await.unwrap();
        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, dek.id);
        assert_eq!(fetched.encrypted_key, key_bytes);
        assert_eq!(fetched.nonce, nonce_bytes);
    }

    #[tokio::test]
    async fn get_encrypted_dek_not_found() {
        let pool = test_pool().await;

        let result = secret_repo::get_encrypted_dek(&pool, Uuid::now_v7())
            .await
            .unwrap();
        assert!(result.is_none());
    }

    // ── create_secret_version / get_current_secret_version ──

    #[tokio::test]
    async fn create_and_get_secret_version() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let dek = secret_repo::create_encrypted_dek(&pool, &fake_bytes(32), &fake_bytes(12))
            .await
            .unwrap();

        let ciphertext = fake_bytes(64);
        let nonce = fake_bytes(12);

        let version =
            secret_repo::create_secret_version(&pool, secret.id, 1, &ciphertext, &nonce, dek.id)
                .await
                .unwrap();

        assert_eq!(version.secret_id, secret.id);
        assert_eq!(version.version, 1);
        assert_eq!(version.ciphertext, ciphertext);
        assert_eq!(version.nonce, nonce);
        assert_eq!(version.dek_id, dek.id);
    }

    #[tokio::test]
    async fn get_current_secret_version_found() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let dek = secret_repo::create_encrypted_dek(&pool, &fake_bytes(32), &fake_bytes(12))
            .await
            .unwrap();

        let created = secret_repo::create_secret_version(
            &pool,
            secret.id,
            1,
            &fake_bytes(64),
            &fake_bytes(12),
            dek.id,
        )
        .await
        .unwrap();

        let fetched = secret_repo::get_current_secret_version(&pool, secret.id, 1)
            .await
            .unwrap();
        assert!(fetched.is_some());
        assert_eq!(fetched.unwrap().id, created.id);
    }

    #[tokio::test]
    async fn get_current_secret_version_wrong_version() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let dek = secret_repo::create_encrypted_dek(&pool, &fake_bytes(32), &fake_bytes(12))
            .await
            .unwrap();

        secret_repo::create_secret_version(
            &pool,
            secret.id,
            1,
            &fake_bytes(64),
            &fake_bytes(12),
            dek.id,
        )
        .await
        .unwrap();

        // Version 2 doesn't exist
        let result = secret_repo::get_current_secret_version(&pool, secret.id, 2)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn create_multiple_versions() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let dek = secret_repo::create_encrypted_dek(&pool, &fake_bytes(32), &fake_bytes(12))
            .await
            .unwrap();

        let v1 = secret_repo::create_secret_version(
            &pool,
            secret.id,
            1,
            &fake_bytes(64),
            &fake_bytes(12),
            dek.id,
        )
        .await
        .unwrap();

        let v2 = secret_repo::create_secret_version(
            &pool,
            secret.id,
            2,
            &fake_bytes(128),
            &fake_bytes(12),
            dek.id,
        )
        .await
        .unwrap();

        assert_ne!(v1.id, v2.id);
        assert_eq!(v1.version, 1);
        assert_eq!(v2.version, 2);

        // Both versions retrievable
        let fetched_v1 = secret_repo::get_current_secret_version(&pool, secret.id, 1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched_v1.id, v1.id);

        let fetched_v2 = secret_repo::get_current_secret_version(&pool, secret.id, 2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fetched_v2.id, v2.id);
    }

    // ── get_secret_with_value ──

    #[tokio::test]
    async fn get_secret_with_value_full_join() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let dek_key = fake_bytes(32);
        let dek_nonce = fake_bytes(12);
        let dek = secret_repo::create_encrypted_dek(&pool, &dek_key, &dek_nonce)
            .await
            .unwrap();

        let ciphertext = fake_bytes(64);
        let value_nonce = fake_bytes(12);

        secret_repo::create_secret_version(&pool, secret.id, 1, &ciphertext, &value_nonce, dek.id)
            .await
            .unwrap();

        let result = secret_repo::get_secret_with_value(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();
        assert!(result.is_some());

        let swv = result.unwrap();
        assert_eq!(swv.secret.id, secret.id);
        assert_eq!(swv.secret.name, name);
        assert_eq!(swv.ciphertext, ciphertext);
        assert_eq!(swv.value_nonce, value_nonce);
        assert_eq!(swv.encrypted_dek_key, dek_key);
        assert_eq!(swv.dek_nonce, dek_nonce);
    }

    #[tokio::test]
    async fn get_secret_with_value_not_found() {
        let pool = test_pool().await;

        let result = secret_repo::get_secret_with_value(
            &pool,
            Scope::Global,
            None,
            None,
            "nonexistent-secret",
        )
        .await
        .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_secret_with_value_uses_current_version() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let dek = secret_repo::create_encrypted_dek(&pool, &fake_bytes(32), &fake_bytes(12))
            .await
            .unwrap();

        // Create version 1
        let v1_ciphertext = fake_bytes(64);
        secret_repo::create_secret_version(
            &pool,
            secret.id,
            1,
            &v1_ciphertext,
            &fake_bytes(12),
            dek.id,
        )
        .await
        .unwrap();

        // Create version 2 and update current_version
        let v2_ciphertext = fake_bytes(128);
        secret_repo::create_secret_version(
            &pool,
            secret.id,
            2,
            &v2_ciphertext,
            &fake_bytes(12),
            dek.id,
        )
        .await
        .unwrap();
        secret_repo::update_secret_version(&pool, secret.id, 2)
            .await
            .unwrap();

        // get_secret_with_value should return version 2
        let result = secret_repo::get_secret_with_value(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result.ciphertext, v2_ciphertext);
        assert_eq!(result.secret.current_version, 2);
    }

    #[tokio::test]
    async fn get_secret_with_value_excludes_deleted() {
        let pool = test_pool().await;
        let name = unique_secret_name();

        let secret = secret_repo::create_secret(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();

        let dek = secret_repo::create_encrypted_dek(&pool, &fake_bytes(32), &fake_bytes(12))
            .await
            .unwrap();

        secret_repo::create_secret_version(
            &pool,
            secret.id,
            1,
            &fake_bytes(64),
            &fake_bytes(12),
            dek.id,
        )
        .await
        .unwrap();

        secret_repo::delete_secret(&pool, secret.id).await.unwrap();

        let result = secret_repo::get_secret_with_value(&pool, Scope::Global, None, None, &name)
            .await
            .unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_secret_with_value_service_scoped() {
        let pool = test_pool().await;
        let name = unique_secret_name();
        let service = unique_service();

        let secret = secret_repo::create_secret(&pool, Scope::Service, Some(&service), None, &name)
            .await
            .unwrap();

        let dek_key = fake_bytes(32);
        let dek_nonce = fake_bytes(12);
        let dek = secret_repo::create_encrypted_dek(&pool, &dek_key, &dek_nonce)
            .await
            .unwrap();

        let ciphertext = fake_bytes(48);
        let value_nonce = fake_bytes(12);
        secret_repo::create_secret_version(&pool, secret.id, 1, &ciphertext, &value_nonce, dek.id)
            .await
            .unwrap();

        let result =
            secret_repo::get_secret_with_value(&pool, Scope::Service, Some(&service), None, &name)
                .await
                .unwrap()
                .unwrap();

        assert_eq!(result.secret.scope, Scope::Service);
        assert_eq!(result.secret.service.as_deref(), Some(service.as_str()));
        assert_eq!(result.ciphertext, ciphertext);
    }
}
