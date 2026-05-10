//! Persistence for `cluster_addons`.

use async_trait::async_trait;
use kubinate_platform::{audit::AuditContext, error::PlatformError};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::model::{AddonStatus, ClusterAddon};

/// CRUD against `cluster_addons`. Tenant scope is enforced by RLS via
/// the `SET LOCAL` issued in every method.
#[async_trait]
pub trait AddonRepository: Send + Sync {
    /// Idempotent insert: returns the existing row if `(cluster, addon)`
    /// already has a live entry, or creates one in `pending` state.
    async fn upsert(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        addon: &str,
        version: &str,
        helm_release: &str,
        audit: &AuditContext,
    ) -> Result<ClusterAddon, PlatformError>;

    /// All live (non-soft-deleted) addons for a cluster.
    async fn list_for_cluster(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<Vec<ClusterAddon>, PlatformError>;

    /// Update the lifecycle state. Used by the runner to mirror
    /// install-workflow progress.
    async fn update_status(
        &self,
        organization_id: Uuid,
        id: Uuid,
        status: AddonStatus,
        reason: Option<&str>,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Fetch a single addon by id within the tenant scope.
    async fn get_by_id(
        &self,
        organization_id: Uuid,
        id: Uuid,
    ) -> Result<Option<ClusterAddon>, PlatformError>;
}

/// Postgres-backed implementation.
pub struct PgAddonRepository {
    pool: PgPool,
}

impl PgAddonRepository {
    /// Wrap a pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AddonRepository for PgAddonRepository {
    async fn upsert(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        addon: &str,
        version: &str,
        helm_release: &str,
        audit: &AuditContext,
    ) -> Result<ClusterAddon, PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;

        // Idempotency: a re-POST returns the existing row instead of
        // colliding on the unique-among-live index. We can't use
        // ON CONFLICT against a partial index, so do an explicit
        // SELECT-then-INSERT inside the same tx.
        let existing = sqlx::query(
            r"
            SELECT id, organization_id, cluster_id, addon, version, helm_release,
                   status, status_reason, created_at, updated_at, version_lock
            FROM cluster_addons
            WHERE cluster_id = $1 AND addon = $2 AND deleted_at IS NULL
            LIMIT 1
            ",
        )
        .bind(cluster_id)
        .bind(addon)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = existing {
            tx.commit().await?;
            return Ok(row_to_addon(row));
        }

        let id = Uuid::now_v7();
        let row = sqlx::query(
            r"
            INSERT INTO cluster_addons
                (id, organization_id, cluster_id, addon, version, helm_release)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING id, organization_id, cluster_id, addon, version, helm_release,
                      status, status_reason, created_at, updated_at, version_lock
            ",
        )
        .bind(id)
        .bind(organization_id)
        .bind(cluster_id)
        .bind(addon)
        .bind(version)
        .bind(helm_release)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row_to_addon(row))
    }

    async fn list_for_cluster(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<Vec<ClusterAddon>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        let rows = sqlx::query(
            r"
            SELECT id, organization_id, cluster_id, addon, version, helm_release,
                   status, status_reason, created_at, updated_at, version_lock
            FROM cluster_addons
            WHERE cluster_id = $1 AND deleted_at IS NULL
            ORDER BY created_at DESC
            ",
        )
        .bind(cluster_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(row_to_addon).collect())
    }

    async fn update_status(
        &self,
        organization_id: Uuid,
        id: Uuid,
        status: AddonStatus,
        reason: Option<&str>,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;
        sqlx::query("UPDATE cluster_addons SET status = $2, status_reason = $3 WHERE id = $1")
            .bind(id)
            .bind(status)
            .bind(reason)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn get_by_id(
        &self,
        organization_id: Uuid,
        id: Uuid,
    ) -> Result<Option<ClusterAddon>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        let row = sqlx::query(
            r"
            SELECT id, organization_id, cluster_id, addon, version, helm_release,
                   status, status_reason, created_at, updated_at, version_lock
            FROM cluster_addons
            WHERE id = $1 AND deleted_at IS NULL
            LIMIT 1
            ",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.map(row_to_addon))
    }
}

async fn set_tenant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    organization_id: Uuid,
) -> Result<(), PlatformError> {
    sqlx::query(&format!(
        "SET LOCAL app.current_tenant_id = '{organization_id}'",
    ))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[allow(clippy::needless_pass_by_value)]
fn row_to_addon(row: sqlx::postgres::PgRow) -> ClusterAddon {
    ClusterAddon {
        id: row.get("id"),
        organization_id: row.get("organization_id"),
        cluster_id: row.get("cluster_id"),
        addon: row.get("addon"),
        version: row.get("version"),
        helm_release: row.get("helm_release"),
        status: row.get("status"),
        status_reason: row.get("status_reason"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        version_lock: row.get("version_lock"),
    }
}
