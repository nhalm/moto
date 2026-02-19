//! Integration tests for audit log repository.
//!
//! These tests hit real `PostgreSQL`. Run with:
//!     cargo test -p moto-keybox-db --features integration
//!
//! Requires the test database to be running:
//!     make test-db-up && make test-db-migrate

#[cfg(feature = "integration")]
mod integration_tests {
    use crate::{AuditEventType, AuditLogQuery, PrincipalType, Scope, audit_repo};
    use moto_test_utils::test_pool;
    use uuid::Uuid;

    /// Generates a unique principal ID for test isolation.
    fn unique_principal_id() -> String {
        format!("test-principal-{}", Uuid::now_v7())
    }

    /// Generates a unique SPIFFE ID for test isolation.
    fn unique_spiffe_id() -> String {
        format!("spiffe://moto.local/garage/test-{}", Uuid::now_v7())
    }

    /// Generates a unique secret name for test isolation.
    fn unique_secret_name() -> String {
        format!("test-secret-{}", Uuid::now_v7())
    }

    // ── insert_audit_entry ──

    #[tokio::test]
    async fn insert_audit_entry_minimal() {
        let pool = test_pool().await;

        let entry = audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::AuthFailed,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(entry.event_type, AuditEventType::AuthFailed);
        assert!(entry.principal_type.is_none());
        assert!(entry.principal_id.is_none());
        assert!(entry.spiffe_id.is_none());
        assert!(entry.secret_scope.is_none());
        assert!(entry.secret_name.is_none());
    }

    #[tokio::test]
    async fn insert_audit_entry_all_fields() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();
        let spiffe_id = unique_spiffe_id();
        let secret_name = unique_secret_name();

        let entry = audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Accessed,
            Some(PrincipalType::Garage),
            Some(&principal_id),
            Some(&spiffe_id),
            Some(Scope::Service),
            Some(&secret_name),
        )
        .await
        .unwrap();

        assert_eq!(entry.event_type, AuditEventType::Accessed);
        assert_eq!(entry.principal_type, Some(PrincipalType::Garage));
        assert_eq!(entry.principal_id.as_deref(), Some(principal_id.as_str()));
        assert_eq!(entry.spiffe_id.as_deref(), Some(spiffe_id.as_str()));
        assert_eq!(entry.secret_scope, Some(Scope::Service));
        assert_eq!(entry.secret_name.as_deref(), Some(secret_name.as_str()));
    }

    #[tokio::test]
    async fn insert_audit_entry_all_event_types() {
        let pool = test_pool().await;

        let event_types = [
            AuditEventType::Accessed,
            AuditEventType::Created,
            AuditEventType::Updated,
            AuditEventType::Deleted,
            AuditEventType::SvidIssued,
            AuditEventType::AuthFailed,
            AuditEventType::AccessDenied,
        ];

        for event_type in event_types {
            let entry =
                audit_repo::insert_audit_entry(&pool, event_type, None, None, None, None, None)
                    .await
                    .unwrap();
            assert_eq!(entry.event_type, event_type);
        }
    }

    // ── list_audit_entries ──

    #[tokio::test]
    async fn list_audit_entries_no_filters() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();

        // Insert a few entries with a unique principal_id so we can find them
        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Created,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Accessed,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let query = AuditLogQuery {
            principal_id: Some(principal_id),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn list_audit_entries_filter_by_event_type() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Created,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Deleted,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let query = AuditLogQuery {
            event_type: Some(AuditEventType::Created),
            principal_id: Some(principal_id),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event_type, AuditEventType::Created);
    }

    #[tokio::test]
    async fn list_audit_entries_filter_by_secret_name() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();
        let secret_a = unique_secret_name();
        let secret_b = unique_secret_name();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Accessed,
            Some(PrincipalType::Garage),
            Some(&principal_id),
            None,
            Some(Scope::Global),
            Some(&secret_a),
        )
        .await
        .unwrap();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Accessed,
            Some(PrincipalType::Garage),
            Some(&principal_id),
            None,
            Some(Scope::Global),
            Some(&secret_b),
        )
        .await
        .unwrap();

        let query = AuditLogQuery {
            secret_name: Some(secret_a.clone()),
            principal_id: Some(principal_id),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].secret_name.as_deref(), Some(secret_a.as_str()));
    }

    #[tokio::test]
    async fn list_audit_entries_ordered_by_timestamp_desc() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();

        // Insert in order: Created, then Accessed
        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Created,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Accessed,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let query = AuditLogQuery {
            principal_id: Some(principal_id),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 2);
        // Most recent first (DESC order)
        assert!(entries[0].timestamp >= entries[1].timestamp);
        assert_eq!(entries[0].event_type, AuditEventType::Accessed);
        assert_eq!(entries[1].event_type, AuditEventType::Created);
    }

    #[tokio::test]
    async fn list_audit_entries_limit_and_offset() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();

        // Insert 3 entries
        for event_type in [
            AuditEventType::Created,
            AuditEventType::Updated,
            AuditEventType::Deleted,
        ] {
            audit_repo::insert_audit_entry(
                &pool,
                event_type,
                Some(PrincipalType::Service),
                Some(&principal_id),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        }

        // Limit to 2
        let query = AuditLogQuery {
            principal_id: Some(principal_id.clone()),
            limit: Some(2),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 2);

        // Offset by 2, should get 1
        let query = AuditLogQuery {
            principal_id: Some(principal_id),
            limit: Some(10),
            offset: Some(2),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn list_audit_entries_empty_result() {
        let pool = test_pool().await;

        let query = AuditLogQuery {
            principal_id: Some(format!("nonexistent-{}", Uuid::now_v7())),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert!(entries.is_empty());
    }

    // ── count_audit_entries ──

    #[tokio::test]
    async fn count_audit_entries_no_filters() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();

        // Insert 3 entries
        for _ in 0..3 {
            audit_repo::insert_audit_entry(
                &pool,
                AuditEventType::Accessed,
                Some(PrincipalType::Garage),
                Some(&principal_id),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        }

        let query = AuditLogQuery {
            principal_id: Some(principal_id),
            ..Default::default()
        };
        let count = audit_repo::count_audit_entries(&pool, &query)
            .await
            .unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn count_audit_entries_with_event_type_filter() {
        let pool = test_pool().await;
        let principal_id = unique_principal_id();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Created,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Deleted,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        audit_repo::insert_audit_entry(
            &pool,
            AuditEventType::Created,
            Some(PrincipalType::Service),
            Some(&principal_id),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        let query = AuditLogQuery {
            event_type: Some(AuditEventType::Created),
            principal_id: Some(principal_id),
            ..Default::default()
        };
        let count = audit_repo::count_audit_entries(&pool, &query)
            .await
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn count_audit_entries_zero() {
        let pool = test_pool().await;

        let query = AuditLogQuery {
            principal_id: Some(format!("nonexistent-{}", Uuid::now_v7())),
            ..Default::default()
        };
        let count = audit_repo::count_audit_entries(&pool, &query)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
