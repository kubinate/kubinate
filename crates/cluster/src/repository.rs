//! Cluster persistence.

use async_trait::async_trait;
use kubinate_platform::{audit::AuditContext, error::PlatformError};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::model::{Cluster, ClusterNodeRole, ClusterServer, ClusterStatus, NewCluster};

/// CRUD for `clusters`. Every mutating method opens a tenant-scoped
/// transaction so the table's RLS + the audit trigger both see the
/// correct `app.current_tenant_id`. Mutators also take an
/// [`AuditContext`] so the trigger captures `actor_user_id`,
/// `request_id`, and friends; reads do not (SELECTs don't fire the
/// trigger).
#[async_trait]
pub trait ClusterRepository: Send + Sync {
    /// Insert a new cluster in `pending` status.
    async fn insert(
        &self,
        organization_id: Uuid,
        new_cluster: NewCluster,
        audit: &AuditContext,
    ) -> Result<Cluster, PlatformError>;

    /// Fetch by id. Returns `NotFound` when missing / soft-deleted.
    async fn get(&self, organization_id: Uuid, id: Uuid) -> Result<Cluster, PlatformError>;

    /// List all live clusters for a tenant, newest first.
    async fn list(&self, organization_id: Uuid) -> Result<Vec<Cluster>, PlatformError>;

    /// Persist the secret-store handle for the cluster's kubeconfig.
    /// Idempotent: re-setting to the same value is a no-op.
    async fn set_kubeconfig_secret(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        secret_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Fetch the kubeconfig secret handle for a cluster, if any has
    /// been recorded. Returns `Ok(None)` when the cluster exists but
    /// has not yet completed provisioning.
    async fn kubeconfig_secret(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<Option<Uuid>, PlatformError>;

    /// Update the cluster's lifecycle state. The audit trigger fires
    /// on the resulting UPDATE so every transition lands in the
    /// per-tenant hash chain.
    async fn update_status(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        status: ClusterStatus,
        reason: Option<&str>,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Update the cluster's `worker_count` after a successful scale.
    /// The audit trigger fires so the change shows up in the hash
    /// chain alongside the workflow's status transitions.
    async fn update_worker_count(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        worker_count: i16,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Soft-delete (set `deleted_at`). Idempotent — a double-delete is
    /// not an error, since the destroy workflow may be re-run.
    async fn soft_delete(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Record a Hetzner server we just created for this cluster, so
    /// the destroy workflow can find it later.
    #[allow(clippy::too_many_arguments)]
    async fn record_server(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        hetzner_server_id: i64,
        role: ClusterNodeRole,
        public_ipv4: Option<&str>,
        private_ipv4: Option<&str>,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// All live (non-soft-deleted) servers for a cluster.
    async fn list_servers(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<Vec<ClusterServer>, PlatformError>;

    /// Mark every server for a cluster as soft-deleted. The destroy
    /// workflow calls this only after Hetzner has confirmed deletion.
    async fn soft_delete_servers(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;
}

/// Postgres-backed implementation.
pub struct PgClusterRepository {
    pool: PgPool,
}

impl PgClusterRepository {
    /// Wrap a pool for repository use.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ClusterRepository for PgClusterRepository {
    async fn insert(
        &self,
        organization_id: Uuid,
        new: NewCluster,
        audit: &AuditContext,
    ) -> Result<Cluster, PlatformError> {
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;

        // Defensive: the form should never let these through, but a
        // hand-crafted request might.
        if new.control_plane_count < 1 {
            return Err(PlatformError::Invalid(
                "control_plane_count must be >= 1".into(),
            ));
        }
        if !(1..=50).contains(&new.worker_count) {
            return Err(PlatformError::Invalid(
                "worker_count must be between 1 and 50".into(),
            ));
        }

        let row = sqlx::query(
            r"
            INSERT INTO clusters
                (id, organization_id, credential_id, name, region, server_type,
                 control_plane_count, worker_count)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, organization_id, credential_id, name, region, server_type,
                      control_plane_count, worker_count, status, status_reason,
                      temporal_workflow_id, created_at, updated_at, version
            ",
        )
        .bind(id)
        .bind(organization_id)
        .bind(new.credential_id)
        .bind(&new.name)
        .bind(&new.region)
        .bind(&new.server_type)
        .bind(new.control_plane_count)
        .bind(new.worker_count)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_conflict)?;

        tx.commit().await?;
        Ok(row_to_cluster(row))
    }

    async fn get(&self, organization_id: Uuid, id: Uuid) -> Result<Cluster, PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;

        let row = sqlx::query(
            r"
            SELECT id, organization_id, credential_id, name, region, server_type,
                   control_plane_count, worker_count, status, status_reason,
                   temporal_workflow_id, created_at, updated_at, version
            FROM clusters
            WHERE id = $1 AND deleted_at IS NULL
            ",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;

        row.map(row_to_cluster)
            .ok_or_else(|| PlatformError::NotFound(format!("cluster/{id}")))
    }

    async fn set_kubeconfig_secret(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        secret_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;
        let affected = sqlx::query(
            r"
            UPDATE clusters
            SET kubeconfig_secret_id = $2
            WHERE id = $1 AND deleted_at IS NULL
              AND (kubeconfig_secret_id IS NULL OR kubeconfig_secret_id = $2)
            ",
        )
        .bind(cluster_id)
        .bind(secret_id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if affected == 0 {
            return Err(PlatformError::Conflict(
                "kubeconfig already set to a different secret".into(),
            ));
        }
        Ok(())
    }

    async fn kubeconfig_secret(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<Option<Uuid>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        let row: Option<(Option<Uuid>,)> = sqlx::query_as(
            "SELECT kubeconfig_secret_id FROM clusters
             WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(cluster_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        match row {
            Some((opt,)) => Ok(opt),
            None => Err(PlatformError::NotFound(format!("cluster/{cluster_id}"))),
        }
    }

    async fn update_status(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        status: ClusterStatus,
        reason: Option<&str>,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;
        let affected = sqlx::query(
            r"
            UPDATE clusters
            SET status = $2, status_reason = $3
            WHERE id = $1
            ",
        )
        .bind(cluster_id)
        .bind(status)
        .bind(reason)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if affected == 0 {
            return Err(PlatformError::NotFound(format!("cluster/{cluster_id}")));
        }
        Ok(())
    }

    async fn update_worker_count(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        worker_count: i16,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;
        let affected = sqlx::query(
            r"
            UPDATE clusters
            SET worker_count = $2
            WHERE id = $1
            ",
        )
        .bind(cluster_id)
        .bind(worker_count)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if affected == 0 {
            return Err(PlatformError::NotFound(format!("cluster/{cluster_id}")));
        }
        Ok(())
    }

    async fn soft_delete(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;
        // Idempotent: the WHERE clause filters to live rows, so
        // re-deleting a destroyed cluster updates zero rows and
        // returns success — exactly what the destroy workflow's
        // re-run AC requires.
        sqlx::query(
            r"
            UPDATE clusters
            SET deleted_at = now()
            WHERE id = $1 AND deleted_at IS NULL
            ",
        )
        .bind(cluster_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list(&self, organization_id: Uuid) -> Result<Vec<Cluster>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        let rows = sqlx::query(
            r"
            SELECT id, organization_id, credential_id, name, region, server_type,
                   control_plane_count, worker_count, status, status_reason,
                   temporal_workflow_id, created_at, updated_at, version
            FROM clusters
            WHERE deleted_at IS NULL
            ORDER BY created_at DESC
            ",
        )
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(row_to_cluster).collect())
    }

    async fn record_server(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        hetzner_server_id: i64,
        role: ClusterNodeRole,
        public_ipv4: Option<&str>,
        private_ipv4: Option<&str>,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;
        sqlx::query(
            r"
            INSERT INTO cluster_servers
                (id, organization_id, cluster_id, hetzner_server_id, role,
                 public_ipv4, private_ipv4)
            VALUES ($1, $2, $3, $4, $5, $6::inet, $7::inet)
            ON CONFLICT (hetzner_server_id) WHERE deleted_at IS NULL DO NOTHING
            ",
        )
        .bind(id)
        .bind(organization_id)
        .bind(cluster_id)
        .bind(hetzner_server_id)
        .bind(role)
        .bind(public_ipv4)
        .bind(private_ipv4)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list_servers(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<Vec<ClusterServer>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        let rows: Vec<(
            Uuid,
            Uuid,
            Uuid,
            i64,
            ClusterNodeRole,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            r"
            SELECT id, organization_id, cluster_id, hetzner_server_id, role,
                   public_ipv4::text, private_ipv4::text
            FROM cluster_servers
            WHERE cluster_id = $1 AND deleted_at IS NULL
            ORDER BY role ASC, created_at ASC
            ",
        )
        .bind(cluster_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|(id, org, cl, hid, role, pubip, privip)| ClusterServer {
                id,
                organization_id: org,
                cluster_id: cl,
                hetzner_server_id: hid,
                role,
                public_ipv4: pubip,
                private_ipv4: privip,
            })
            .collect())
    }

    async fn soft_delete_servers(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        set_tenant(&mut tx, organization_id).await?;
        audit.apply(&mut tx).await?;
        sqlx::query(
            "UPDATE cluster_servers SET deleted_at = now()
             WHERE cluster_id = $1 AND deleted_at IS NULL",
        )
        .bind(cluster_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
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
fn row_to_cluster(row: sqlx::postgres::PgRow) -> Cluster {
    Cluster {
        id: row.get("id"),
        organization_id: row.get("organization_id"),
        credential_id: row.get("credential_id"),
        name: row.get("name"),
        region: row.get("region"),
        server_type: row.get("server_type"),
        control_plane_count: row.get("control_plane_count"),
        worker_count: row.get("worker_count"),
        status: row.get("status"),
        status_reason: row.get("status_reason"),
        temporal_workflow_id: row.get("temporal_workflow_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        version: row.get("version"),
    }
}

fn map_conflict(err: sqlx::Error) -> PlatformError {
    if let sqlx::Error::Database(db_err) = &err {
        if db_err.is_unique_violation() {
            return PlatformError::Conflict("a cluster with this name already exists".into());
        }
    }
    PlatformError::Database(err)
}

// `ClusterStatus` is currently unused by public callers but is
// exercised through row decoding; silence the lint until the service
// layer starts returning it to the API shape.
#[allow(dead_code)]
fn _force_cluster_status_in_use(_: ClusterStatus) {}
