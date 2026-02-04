//! Secret repository for `PostgreSQL`.
//!
//! Provides CRUD operations for secrets and their versions using `PostgreSQL`.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::{DbPool, DbResult, EncryptedDek, Scope, Secret, SecretVersion};

/// Creates a new secret in the database.
///
/// # Errors
///
/// Returns an error if the insert fails (e.g., duplicate key).
pub async fn create_secret(
    pool: &DbPool,
    scope: Scope,
    service: Option<&str>,
    instance_id: Option<&str>,
    name: &str,
) -> DbResult<Secret> {
    let row = sqlx::query_as::<_, Secret>(
        r"
        INSERT INTO secrets (scope, service, instance_id, name)
        VALUES ($1, $2, $3, $4)
        RETURNING *
        ",
    )
    .bind(scope)
    .bind(service)
    .bind(instance_id)
    .bind(name)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Gets a secret by scope, service, `instance_id`, and name.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_secret(
    pool: &DbPool,
    scope: Scope,
    service: Option<&str>,
    instance_id: Option<&str>,
    name: &str,
) -> DbResult<Option<Secret>> {
    let row = sqlx::query_as::<_, Secret>(
        r"
        SELECT * FROM secrets
        WHERE scope = $1
          AND (service IS NOT DISTINCT FROM $2)
          AND (instance_id IS NOT DISTINCT FROM $3)
          AND name = $4
          AND deleted_at IS NULL
        ",
    )
    .bind(scope)
    .bind(service)
    .bind(instance_id)
    .bind(name)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Gets a secret by its UUID.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_secret_by_id(pool: &DbPool, id: Uuid) -> DbResult<Option<Secret>> {
    let row = sqlx::query_as::<_, Secret>(
        r"
        SELECT * FROM secrets
        WHERE id = $1 AND deleted_at IS NULL
        ",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Lists all secrets in a scope.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_secrets(pool: &DbPool, scope: Scope) -> DbResult<Vec<Secret>> {
    let rows = sqlx::query_as::<_, Secret>(
        r"
        SELECT * FROM secrets
        WHERE scope = $1 AND deleted_at IS NULL
        ORDER BY name
        ",
    )
    .bind(scope)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Lists all secrets for a specific service.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_service_secrets(pool: &DbPool, service: &str) -> DbResult<Vec<Secret>> {
    let rows = sqlx::query_as::<_, Secret>(
        r"
        SELECT * FROM secrets
        WHERE scope = $1 AND service = $2 AND deleted_at IS NULL
        ORDER BY name
        ",
    )
    .bind(Scope::Service)
    .bind(service)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Lists all secrets for a specific instance.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn list_instance_secrets(pool: &DbPool, instance_id: &str) -> DbResult<Vec<Secret>> {
    let rows = sqlx::query_as::<_, Secret>(
        r"
        SELECT * FROM secrets
        WHERE scope = $1 AND instance_id = $2 AND deleted_at IS NULL
        ORDER BY name
        ",
    )
    .bind(Scope::Instance)
    .bind(instance_id)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

/// Updates the current version of a secret.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn update_secret_version(pool: &DbPool, id: Uuid, new_version: i32) -> DbResult<Secret> {
    let row = sqlx::query_as::<_, Secret>(
        r"
        UPDATE secrets
        SET current_version = $2, updated_at = now()
        WHERE id = $1
        RETURNING *
        ",
    )
    .bind(id)
    .bind(new_version)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Soft-deletes a secret.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn delete_secret(pool: &DbPool, id: Uuid) -> DbResult<()> {
    sqlx::query(
        r"
        UPDATE secrets
        SET deleted_at = now()
        WHERE id = $1
        ",
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Creates an encrypted DEK in the database.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub async fn create_encrypted_dek(
    pool: &DbPool,
    encrypted_key: &[u8],
    nonce: &[u8],
) -> DbResult<EncryptedDek> {
    let row = sqlx::query_as::<_, EncryptedDek>(
        r"
        INSERT INTO encrypted_deks (encrypted_key, nonce)
        VALUES ($1, $2)
        RETURNING *
        ",
    )
    .bind(encrypted_key)
    .bind(nonce)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Gets an encrypted DEK by its UUID.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_encrypted_dek(pool: &DbPool, id: Uuid) -> DbResult<Option<EncryptedDek>> {
    let row = sqlx::query_as::<_, EncryptedDek>(
        r"
        SELECT * FROM encrypted_deks
        WHERE id = $1
        ",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Creates a new secret version in the database.
///
/// # Errors
///
/// Returns an error if the insert fails.
pub async fn create_secret_version(
    pool: &DbPool,
    secret_id: Uuid,
    version: i32,
    ciphertext: &[u8],
    nonce: &[u8],
    dek_id: Uuid,
) -> DbResult<SecretVersion> {
    let row = sqlx::query_as::<_, SecretVersion>(
        r"
        INSERT INTO secret_versions (secret_id, version, ciphertext, nonce, dek_id)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        ",
    )
    .bind(secret_id)
    .bind(version)
    .bind(ciphertext)
    .bind(nonce)
    .bind(dek_id)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Gets the current version of a secret.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_current_secret_version(
    pool: &DbPool,
    secret_id: Uuid,
    version: i32,
) -> DbResult<Option<SecretVersion>> {
    let row = sqlx::query_as::<_, SecretVersion>(
        r"
        SELECT * FROM secret_versions
        WHERE secret_id = $1 AND version = $2
        ",
    )
    .bind(secret_id)
    .bind(version)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Secret with its encrypted value for retrieval.
#[derive(Debug)]
pub struct SecretWithValue {
    /// The secret metadata.
    pub secret: Secret,
    /// The encrypted secret value (ciphertext).
    pub ciphertext: Vec<u8>,
    /// The secret value nonce.
    pub value_nonce: Vec<u8>,
    /// The encrypted DEK.
    pub encrypted_dek_key: Vec<u8>,
    /// The DEK nonce.
    pub dek_nonce: Vec<u8>,
}

/// Gets a secret with its encrypted value in a single query.
///
/// # Errors
///
/// Returns an error if the query fails.
pub async fn get_secret_with_value(
    pool: &DbPool,
    scope: Scope,
    service: Option<&str>,
    instance_id: Option<&str>,
    name: &str,
) -> DbResult<Option<SecretWithValue>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        // Secret fields
        id: Uuid,
        scope: Scope,
        service: Option<String>,
        instance_id: Option<String>,
        name: String,
        current_version: i32,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        deleted_at: Option<DateTime<Utc>>,
        // SecretVersion fields
        ciphertext: Vec<u8>,
        value_nonce: Vec<u8>,
        // EncryptedDek fields
        encrypted_dek_key: Vec<u8>,
        dek_nonce: Vec<u8>,
    }

    let row = sqlx::query_as::<_, Row>(
        r"
        SELECT
            s.id, s.scope, s.service, s.instance_id, s.name,
            s.current_version, s.created_at, s.updated_at, s.deleted_at,
            sv.ciphertext, sv.nonce as value_nonce,
            ed.encrypted_key as encrypted_dek_key, ed.nonce as dek_nonce
        FROM secrets s
        JOIN secret_versions sv ON sv.secret_id = s.id AND sv.version = s.current_version
        JOIN encrypted_deks ed ON ed.id = sv.dek_id
        WHERE s.scope = $1
          AND (s.service IS NOT DISTINCT FROM $2)
          AND (s.instance_id IS NOT DISTINCT FROM $3)
          AND s.name = $4
          AND s.deleted_at IS NULL
        ",
    )
    .bind(scope)
    .bind(service)
    .bind(instance_id)
    .bind(name)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| SecretWithValue {
        secret: Secret {
            id: r.id,
            scope: r.scope,
            service: r.service,
            instance_id: r.instance_id,
            name: r.name,
            current_version: r.current_version,
            created_at: r.created_at,
            updated_at: r.updated_at,
            deleted_at: r.deleted_at,
        },
        ciphertext: r.ciphertext,
        value_nonce: r.value_nonce,
        encrypted_dek_key: r.encrypted_dek_key,
        dek_nonce: r.dek_nonce,
    }))
}
