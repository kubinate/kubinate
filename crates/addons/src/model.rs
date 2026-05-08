//! Domain types for `cluster_addons`.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Lifecycle state mirroring the SQL `addon_status` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "addon_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AddonStatus {
    /// Row created, install workflow not yet started.
    Pending,
    /// Helm install in flight.
    Installing,
    /// Chart deployed and all replicas ready.
    Ready,
    /// Workflow failed; manual intervention required.
    Failed,
    /// Removal workflow in flight (Sprint 3).
    Uninstalling,
    /// Removed; row retained for audit.
    Uninstalled,
}

/// Persisted cluster-addon row.
#[derive(Debug, Clone, Serialize)]
pub struct ClusterAddon {
    /// Primary key.
    pub id: Uuid,
    /// Owning tenant.
    pub organization_id: Uuid,
    /// Cluster this addon is installed on.
    pub cluster_id: Uuid,
    /// Catalog slug.
    pub addon: String,
    /// Helm chart version pinned for this install.
    pub version: String,
    /// Helm release name actually used.
    pub helm_release: String,
    /// Lifecycle state.
    pub status: AddonStatus,
    /// Human-readable last-known status detail.
    pub status_reason: Option<String>,
    /// Row creation time.
    pub created_at: OffsetDateTime,
    /// Row last-mutation time.
    pub updated_at: OffsetDateTime,
    /// Optimistic-lock counter (renamed to avoid clashing with the
    /// chart `version` column).
    pub version_lock: i64,
}

/// Input the API hands to the service when a user requests an install.
#[derive(Debug, Clone, Deserialize)]
pub struct NewAddon {
    /// Catalog slug — must be on the allowlist.
    pub addon: String,
    /// Pinned chart version. Phase 1 trusts the user; Sprint 3 may
    /// validate against an upstream-version index.
    pub version: String,
}
