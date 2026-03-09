//! Integration tests for audit log repository.
//!
//! These tests hit real `PostgreSQL`. Run with:
//!     cargo test -p moto-keybox-db --features integration
//!
//! Requires the test database to be running:
//!     make test-db-up && make test-db-migrate

#[cfg(feature = "integration")]
mod integration_tests {
    use crate::{AuditEventType, AuditLogQuery, PrincipalType, audit_repo};
    use audit_repo::InsertAuditEntry;
    use moto_test_utils::test_pool;
    use uuid::Uuid;

    /// Generates a unique principal ID for test isolation.
    fn unique_principal_id() -> String {
        format!("spiffe://moto.local/garage/test-{}", Uuid::now_v7())
    }

    fn test_entry(principal_id: &str) -> InsertAuditEntry<'_> {
        InsertAuditEntry {
            event_type: AuditEventType::SecretAccessed,
            service: "keybox",
            principal_type: PrincipalType::Garage,
            principal_id,
            action: "read",
            resource_type: "secret",
            resource_id: "global/test-secret",
            outcome: "success",
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            client_ip: None,
        }
    }

    // ── insert_audit_entry ──

    #[tokio::test]
    async fn insert_audit_entry_minimal() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        let entry = InsertAuditEntry {
            event_type: AuditEventType::AuthFailed,
            service: "keybox",
            principal_type: PrincipalType::Service,
            principal_id: &pid,
            action: "auth_fail",
            resource_type: "token",
            resource_id: "",
            outcome: "denied",
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            client_ip: None,
        };

        let result = audit_repo::insert_audit_entry(&pool, &entry).await.unwrap();

        assert_eq!(result.event_type, AuditEventType::AuthFailed);
        assert_eq!(result.service, "keybox");
        assert_eq!(result.action, "auth_fail");
        assert_eq!(result.outcome, "denied");
        assert!(result.client_ip.is_none());
    }

    #[tokio::test]
    async fn insert_audit_entry_all_fields() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        let entry = InsertAuditEntry {
            event_type: AuditEventType::SecretAccessed,
            service: "keybox",
            principal_type: PrincipalType::Garage,
            principal_id: &pid,
            action: "read",
            resource_type: "secret",
            resource_id: "global/ai/anthropic",
            outcome: "success",
            metadata: serde_json::json!({"scope": "global"}),
            client_ip: Some("10.42.0.15"),
        };

        let result = audit_repo::insert_audit_entry(&pool, &entry).await.unwrap();

        assert_eq!(result.event_type, AuditEventType::SecretAccessed);
        assert_eq!(result.principal_type, PrincipalType::Garage);
        assert_eq!(result.principal_id, pid);
        assert_eq!(result.resource_type, "secret");
        assert_eq!(result.resource_id, "global/ai/anthropic");
        assert_eq!(result.client_ip.as_deref(), Some("10.42.0.15"));
    }

    #[tokio::test]
    async fn insert_audit_entry_all_event_types() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        let event_types = [
            AuditEventType::SecretAccessed,
            AuditEventType::SecretCreated,
            AuditEventType::SecretUpdated,
            AuditEventType::SecretDeleted,
            AuditEventType::SvidIssued,
            AuditEventType::AuthFailed,
            AuditEventType::AccessDenied,
        ];

        for event_type in event_types {
            let entry = InsertAuditEntry {
                event_type,
                service: "keybox",
                principal_type: PrincipalType::Service,
                principal_id: &pid,
                action: "test",
                resource_type: "test",
                resource_id: "",
                outcome: "success",
                metadata: serde_json::Value::Object(serde_json::Map::new()),
                client_ip: None,
            };
            let result = audit_repo::insert_audit_entry(&pool, &entry).await.unwrap();
            assert_eq!(result.event_type, event_type);
        }
    }

    // ── list_audit_entries ──

    #[tokio::test]
    async fn list_audit_entries_no_filters() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        let entry1 = InsertAuditEntry {
            event_type: AuditEventType::SecretCreated,
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry1)
            .await
            .unwrap();

        let entry2 = InsertAuditEntry {
            event_type: AuditEventType::SecretAccessed,
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry2)
            .await
            .unwrap();

        let query = AuditLogQuery {
            principal_id: Some(pid),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn list_audit_entries_filter_by_event_type() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        let entry1 = InsertAuditEntry {
            event_type: AuditEventType::SecretCreated,
            action: "create",
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry1)
            .await
            .unwrap();

        let entry2 = InsertAuditEntry {
            event_type: AuditEventType::SecretDeleted,
            action: "delete",
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry2)
            .await
            .unwrap();

        let query = AuditLogQuery {
            event_type: Some(AuditEventType::SecretCreated),
            principal_id: Some(pid),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event_type, AuditEventType::SecretCreated);
    }

    #[tokio::test]
    async fn list_audit_entries_filter_by_resource_type() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        audit_repo::insert_audit_entry(&pool, &test_entry(&pid))
            .await
            .unwrap();

        let svid_entry = InsertAuditEntry {
            event_type: AuditEventType::SvidIssued,
            action: "create",
            resource_type: "svid",
            resource_id: &pid,
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &svid_entry)
            .await
            .unwrap();

        let query = AuditLogQuery {
            resource_type: Some("secret".to_string()),
            principal_id: Some(pid),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].resource_type, "secret");
    }

    #[tokio::test]
    async fn list_audit_entries_ordered_by_timestamp_desc() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        let entry1 = InsertAuditEntry {
            event_type: AuditEventType::SecretCreated,
            action: "create",
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry1)
            .await
            .unwrap();

        let entry2 = InsertAuditEntry {
            event_type: AuditEventType::SecretAccessed,
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry2)
            .await
            .unwrap();

        let query = AuditLogQuery {
            principal_id: Some(pid),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].timestamp >= entries[1].timestamp);
        assert_eq!(entries[0].event_type, AuditEventType::SecretAccessed);
        assert_eq!(entries[1].event_type, AuditEventType::SecretCreated);
    }

    #[tokio::test]
    async fn list_audit_entries_limit_and_offset() {
        let pool = test_pool().await;
        let pid = unique_principal_id();

        for event_type in [
            AuditEventType::SecretCreated,
            AuditEventType::SecretUpdated,
            AuditEventType::SecretDeleted,
        ] {
            let entry = InsertAuditEntry {
                event_type,
                ..test_entry(&pid)
            };
            audit_repo::insert_audit_entry(&pool, &entry).await.unwrap();
        }

        let query = AuditLogQuery {
            principal_id: Some(pid.clone()),
            limit: Some(2),
            ..Default::default()
        };
        let entries = audit_repo::list_audit_entries(&pool, &query).await.unwrap();
        assert_eq!(entries.len(), 2);

        let query = AuditLogQuery {
            principal_id: Some(pid),
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
        let pid = unique_principal_id();

        for _ in 0..3 {
            audit_repo::insert_audit_entry(&pool, &test_entry(&pid))
                .await
                .unwrap();
        }

        let query = AuditLogQuery {
            principal_id: Some(pid),
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
        let pid = unique_principal_id();

        let entry1 = InsertAuditEntry {
            event_type: AuditEventType::SecretCreated,
            action: "create",
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry1)
            .await
            .unwrap();

        let entry2 = InsertAuditEntry {
            event_type: AuditEventType::SecretDeleted,
            action: "delete",
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry2)
            .await
            .unwrap();

        let entry3 = InsertAuditEntry {
            event_type: AuditEventType::SecretCreated,
            action: "create",
            ..test_entry(&pid)
        };
        audit_repo::insert_audit_entry(&pool, &entry3)
            .await
            .unwrap();

        let query = AuditLogQuery {
            event_type: Some(AuditEventType::SecretCreated),
            principal_id: Some(pid),
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
