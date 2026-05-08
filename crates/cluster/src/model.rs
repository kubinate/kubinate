//! Cluster domain types.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Lifecycle state of a cluster; mirrors the `cluster_status` SQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "cluster_status", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ClusterStatus {
    /// Row created, workflow not yet kicked off.
    Pending,
    /// Temporal workflow is running.
    Provisioning,
    /// Provisioning complete.
    Ready,
    /// Scale-out / scale-in workflow in flight (Sprint 3 ticket 08).
    Scaling,
    /// Workflow failed; cluster is in an unknown or partial state.
    Failed,
    /// Teardown in flight.
    Destroying,
    /// Teardown complete. Row retained for audit trail.
    Destroyed,
}

/// Persisted cluster row.
#[derive(Debug, Clone, Serialize)]
pub struct Cluster {
    /// Primary key.
    pub id: Uuid,
    /// Owning tenant.
    pub organization_id: Uuid,
    /// Hetzner credential this cluster was created against.
    pub credential_id: Uuid,
    /// User-visible name (unique per org among live rows).
    pub name: String,
    /// Hetzner region slug (e.g. `nbg1`).
    pub region: String,
    /// Hetzner server type (e.g. `cpx21`).
    pub server_type: String,
    /// Control-plane node count. Sprint 1 is always 1.
    pub control_plane_count: i16,
    /// Worker node count.
    pub worker_count: i16,
    /// Current lifecycle state.
    pub status: ClusterStatus,
    /// Human-readable last-known status detail.
    pub status_reason: Option<String>,
    /// Temporal workflow id once started.
    pub temporal_workflow_id: Option<String>,
    /// Row creation time.
    pub created_at: OffsetDateTime,
    /// Row last-mutation time.
    pub updated_at: OffsetDateTime,
    /// Optimistic-lock version.
    pub version: i64,
}

/// Input used to create a new cluster. The API layer validates the
/// incoming JSON body and then constructs one of these.
#[derive(Debug, Clone)]
pub struct NewCluster {
    /// User-visible name.
    pub name: String,
    /// Hetzner region slug.
    pub region: String,
    /// Hetzner server type slug.
    pub server_type: String,
    /// Control plane count.
    pub control_plane_count: i16,
    /// Worker node count.
    pub worker_count: i16,
    /// Reference to an existing `hetzner_credentials` row.
    pub credential_id: Uuid,
}

/// Role a node plays in a cluster — mirrors the SQL `cluster_node_role` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "cluster_node_role", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ClusterNodeRole {
    /// Single-node control plane in Sprint 1; HA in Phase 2.
    ControlPlane,
    /// Worker node.
    Worker,
}

/// One Hetzner server attached to a cluster. The destroy workflow
/// reads this list to build its server-id input.
#[derive(Debug, Clone)]
pub struct ClusterServer {
    /// Primary key.
    pub id: Uuid,
    /// Owning tenant.
    pub organization_id: Uuid,
    /// Cluster this server belongs to.
    pub cluster_id: Uuid,
    /// Hetzner Cloud server id.
    pub hetzner_server_id: i64,
    /// Role within the cluster.
    pub role: ClusterNodeRole,
    /// Public IPv4 if Hetzner has assigned one.
    pub public_ipv4: Option<String>,
    /// Private IPv4 within the cluster's Hetzner network.
    pub private_ipv4: Option<String>,
}
