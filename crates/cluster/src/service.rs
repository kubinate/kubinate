//! Cluster use-cases.

use std::sync::Arc;

use kubinate_platform::{
    audit::AuditContext,
    error::PlatformError,
    secrets::{SecretRef, SecretStore},
};
use secrecy::SecretString;
use uuid::Uuid;

use crate::{
    model::{Cluster, ClusterStatus, NewCluster},
    repository::ClusterRepository,
};

/// High-level operations over the cluster bounded context.
///
/// Sprint 1 scope: create (pending status) and read. The Temporal
/// trigger that transitions `pending → provisioning` lives in ticket
/// 03; this service stays workflow-agnostic so the integration keeps
/// a clean seam.
pub struct ClusterService {
    repo: Arc<dyn ClusterRepository>,
    secrets: Arc<dyn SecretStore>,
}

impl ClusterService {
    /// Wire the service with its repository and the secret store used
    /// for envelope-encrypted kubeconfig storage (ticket 04).
    #[must_use]
    pub fn new(repo: Arc<dyn ClusterRepository>, secrets: Arc<dyn SecretStore>) -> Self {
        Self { repo, secrets }
    }

    /// Create a new cluster row in `pending` status. Validates the
    /// allowlisted server type and region so the workflow never runs
    /// against free-form user input.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if validation fails or the database query fails.
    pub async fn create(
        &self,
        organization_id: Uuid,
        new_cluster: NewCluster,
        audit: &AuditContext,
    ) -> Result<Cluster, PlatformError> {
        validate_allowed(&new_cluster)?;
        self.repo.insert(organization_id, new_cluster, audit).await
    }

    /// Fetch by id (for the dashboard status page).
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the cluster is not found or the database query fails.
    pub async fn get(&self, organization_id: Uuid, id: Uuid) -> Result<Cluster, PlatformError> {
        self.repo.get(organization_id, id).await
    }

    /// List live clusters for a tenant.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the database query fails.
    pub async fn list(&self, organization_id: Uuid) -> Result<Vec<Cluster>, PlatformError> {
        self.repo.list(organization_id).await
    }

    /// Encrypt and persist the kubeconfig produced by the
    /// provisioning workflow. Idempotent — a second call with the
    /// same plaintext re-uses the existing handle on the cluster row.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the secret store or the database query fails.
    pub async fn store_kubeconfig(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        kubeconfig: SecretString,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let secret_ref = self.secrets.put(organization_id, kubeconfig).await?;
        match self
            .repo
            .set_kubeconfig_secret(organization_id, cluster_id, secret_ref.id, audit)
            .await
        {
            Ok(()) => Ok(()),
            Err(err) => {
                // Cluster row update failed — best-effort cleanup so we
                // don't leave an orphaned ciphertext blob behind.
                if let Err(cleanup) = self.secrets.delete(&secret_ref).await {
                    tracing::warn!(
                        error = %cleanup,
                        secret_id = %secret_ref.id,
                        "failed to clean up orphaned kubeconfig secret",
                    );
                }
                Err(err)
            }
        }
    }

    /// Retrieve the stored kubeconfig plaintext. The caller is
    /// responsible for keeping the returned [`SecretString`] inside
    /// the request handler — never log it, never persist it elsewhere.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the kubeconfig is not yet available or the
    /// database query fails.
    pub async fn fetch_kubeconfig(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<SecretString, PlatformError> {
        let Some(secret_id) = self
            .repo
            .kubeconfig_secret(organization_id, cluster_id)
            .await?
        else {
            return Err(PlatformError::NotFound(format!(
                "kubeconfig for cluster/{cluster_id} not yet available",
            )));
        };
        let handle = SecretRef {
            id: secret_id,
            organization_id,
        };
        Ok(self.secrets.get(&handle).await?)
    }

    /// Mark a cluster `destroying`. Called immediately before the
    /// destroy workflow kicks off so the UI can reflect the in-flight
    /// state. Subsequent failures still leave the cluster recoverable
    /// (a re-run completes per ticket 05's idempotency AC).
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the cluster is already destroyed or the
    /// database query fails.
    pub async fn begin_destroy(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        audit: &AuditContext,
    ) -> Result<Cluster, PlatformError> {
        let cluster = self.repo.get(organization_id, cluster_id).await?;
        if matches!(cluster.status, ClusterStatus::Destroyed) {
            return Err(PlatformError::Conflict(
                "cluster is already destroyed".into(),
            ));
        }
        self.repo
            .update_status(
                organization_id,
                cluster_id,
                ClusterStatus::Destroying,
                None,
                audit,
            )
            .await?;
        Ok(cluster)
    }

    /// Run after the destroy workflow returns successfully: zero out
    /// the kubeconfig secret, soft-delete the cluster row, and flip
    /// the status to `destroyed`. Each step is independently
    /// idempotent so a second call after a partial failure converges.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the database query fails.
    pub async fn finalize_destroy(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        if let Some(secret_id) = self
            .repo
            .kubeconfig_secret(organization_id, cluster_id)
            .await?
        {
            // Best-effort — if the secret is already gone, log and move
            // on. The cluster row update must still happen.
            let handle = SecretRef {
                id: secret_id,
                organization_id,
            };
            if let Err(err) = self.secrets.delete(&handle).await {
                tracing::warn!(
                    error = %err,
                    secret_id = %secret_id,
                    cluster_id = %cluster_id,
                    "failed to delete kubeconfig secret during destroy",
                );
            }
        }

        self.repo
            .soft_delete(organization_id, cluster_id, audit)
            .await?;
        self.repo
            .update_status(
                organization_id,
                cluster_id,
                ClusterStatus::Destroyed,
                None,
                audit,
            )
            .await?;
        Ok(())
    }

    /// Mark a destroy attempt as failed (e.g. unrecoverable Hetzner
    /// auth error). Caller's responsibility to call this from the
    /// runner's error path.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the database query fails.
    pub async fn mark_destroy_failed(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        reason: &str,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        self.repo
            .update_status(
                organization_id,
                cluster_id,
                ClusterStatus::Failed,
                Some(reason),
                audit,
            )
            .await
    }
}

/// Sprint 1 regions — Hetzner's current public set. Extended as
/// operational coverage grows.
pub const ALLOWED_REGIONS: &[&str] = &["nbg1", "fsn1", "hel1", "ash", "hil"];

/// Server types we permit through the form. Keeping this narrow during
/// the thin slice avoids accidentally provisioning against capacity
/// classes the workflow has not been tested on.
pub const ALLOWED_SERVER_TYPES: &[&str] = &[
    "cpx11", "cpx21", "cpx31", "cpx41", // shared vCPU
    "ccx13", "ccx23", // dedicated vCPU
];

fn validate_allowed(new_cluster: &NewCluster) -> Result<(), PlatformError> {
    if new_cluster.name.trim().is_empty() {
        return Err(PlatformError::Invalid("name must not be empty".into()));
    }
    if !ALLOWED_REGIONS.contains(&new_cluster.region.as_str()) {
        return Err(PlatformError::Invalid(format!(
            "region '{}' is not on the allowlist",
            new_cluster.region
        )));
    }
    if !ALLOWED_SERVER_TYPES.contains(&new_cluster.server_type.as_str()) {
        return Err(PlatformError::Invalid(format!(
            "server_type '{}' is not on the allowlist",
            new_cluster.server_type
        )));
    }
    if new_cluster.control_plane_count != 1 {
        // ADR-0012 defers HA control-plane delivery to Phase 4+; the
        // single-CP invariant is now load-bearing rather than a
        // Sprint-1 placeholder. The error points the caller at the
        // revisit triggers so they know what condition unblocks
        // multi-CP rather than just hitting "no" with no context.
        return Err(PlatformError::Invalid(
            "control_plane_count must be 1 — HA control-plane delivery \
             is deferred to Phase 4+ (see ADR-0012). Revisit triggers: \
             a data-residency customer that requires multi-region, or \
             Hetzner shipping a managed etcd / Cluster API offering."
                .into(),
        ));
    }
    if !(1..=10).contains(&new_cluster.worker_count) {
        return Err(PlatformError::Invalid(
            "worker_count must be between 1 and 10".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_new() -> NewCluster {
        NewCluster {
            name: "prod".into(),
            region: "nbg1".into(),
            server_type: "cpx21".into(),
            control_plane_count: 1,
            worker_count: 3,
            credential_id: Uuid::now_v7(),
        }
    }

    #[test]
    fn rejects_unknown_region() {
        let mut nc = base_new();
        nc.region = "mars1".into();
        let err = validate_allowed(&nc).unwrap_err();
        assert!(matches!(err, PlatformError::Invalid(_)));
    }

    #[test]
    fn rejects_unknown_server_type() {
        let mut nc = base_new();
        nc.server_type = "xxl-not-real".into();
        let err = validate_allowed(&nc).unwrap_err();
        assert!(matches!(err, PlatformError::Invalid(_)));
    }

    #[test]
    fn rejects_multi_control_plane_with_adr_pointer() {
        // ADR-0012 ratified the single-CP invariant; the message
        // must point future contributors at it so the rejection
        // reads as a deliberate decision, not a TODO.
        let mut nc = base_new();
        nc.control_plane_count = 3;
        let err = validate_allowed(&nc).unwrap_err();
        match err {
            PlatformError::Invalid(msg) => {
                assert!(msg.contains("ADR-0012"), "missing ADR pointer: {msg}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn rejects_worker_count_out_of_range() {
        let mut nc = base_new();
        nc.worker_count = 42;
        let err = validate_allowed(&nc).unwrap_err();
        assert!(matches!(err, PlatformError::Invalid(_)));
    }

    #[test]
    fn accepts_valid_request() {
        assert!(validate_allowed(&base_new()).is_ok());
    }
}
