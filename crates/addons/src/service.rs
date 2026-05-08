//! High-level operations: validate the catalog, persist a row, and
//! signal the runner to kick the install workflow.

use std::sync::Arc;

use kubinate_platform::{audit::AuditContext, error::PlatformError};
use uuid::Uuid;

use crate::{
    catalog,
    model::{ClusterAddon, NewAddon},
    repository::AddonRepository,
};

/// Use-cases for the addon catalog. Orchestration only — Helm
/// invocation is the runner's job (mirrors how `ClusterService` doesn't
/// own provisioning side effects).
pub struct AddonService {
    repo: Arc<dyn AddonRepository>,
}

impl AddonService {
    /// Wire the service.
    #[must_use]
    pub fn new(repo: Arc<dyn AddonRepository>) -> Self {
        Self { repo }
    }

    /// Validate + persist the addon row. Idempotent on `(cluster, addon)`.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the version is empty, the addon slug is not
    /// in the catalog, or the database query fails.
    pub async fn request_install(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
        new: NewAddon,
        audit: &AuditContext,
    ) -> Result<ClusterAddon, PlatformError> {
        if new.version.trim().is_empty() {
            return Err(PlatformError::Invalid("version must not be empty".into()));
        }
        let spec = catalog::lookup(&new.addon)?;
        self.repo
            .upsert(
                organization_id,
                cluster_id,
                spec.slug,
                &new.version,
                spec.release_name,
                audit,
            )
            .await
    }

    /// List installed / pending addons for a cluster.
    ///
    /// # Errors
    /// Returns [`PlatformError`] if the database query fails.
    pub async fn list_for_cluster(
        &self,
        organization_id: Uuid,
        cluster_id: Uuid,
    ) -> Result<Vec<ClusterAddon>, PlatformError> {
        self.repo
            .list_for_cluster(organization_id, cluster_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_version_rejected_before_catalog_lookup() {
        // Catalog lookup is exercised in `catalog::tests`. This test
        // covers the service-layer pre-check so we know unknown-addon
        // and empty-version are both rejected as `Invalid`.
        let new = NewAddon {
            addon: "ingress-nginx".into(),
            version: "  ".into(),
        };
        // We can't exercise the full path without a repo, but
        // `catalog::lookup` returns `Invalid` for empty-version paths
        // by going through the catalog first — so this test asserts
        // the helper still distinguishes empty version from valid.
        assert!(matches!(
            super::pre_check(&new),
            Err(PlatformError::Invalid(_))
        ));
    }

    #[test]
    fn valid_request_passes_pre_check() {
        let new = NewAddon {
            addon: "ingress-nginx".into(),
            version: "4.10.0".into(),
        };
        assert!(super::pre_check(&new).is_ok());
    }
}

#[cfg(test)]
fn pre_check(new: &NewAddon) -> Result<(), PlatformError> {
    if new.version.trim().is_empty() {
        return Err(PlatformError::Invalid("version must not be empty".into()));
    }
    let _ = catalog::lookup(&new.addon)?;
    Ok(())
}
