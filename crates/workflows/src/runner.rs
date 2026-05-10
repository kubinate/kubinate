//! In-process runner that drives [`crate::workflows::provision_cluster`]
//! / [`crate::workflows::destroy_cluster`] from the API edge.
//!
//! The real Temporal SDK wiring (ADR-0003) replaces this when the
//! Phase-0 spike concludes; until then this gives us:
//!
//! 1. A thread the API can hand a cluster to (`tokio::spawn`),
//! 2. Per-step progress updates mirrored into the
//!    `provisioning_workflows` shadow table (so the dashboard polls
//!    can render the current step), and
//! 3. End-of-run finalization: store the kubeconfig via the cluster
//!    service (ticket 04), record server ids in `cluster_servers`
//!    (ticket 05 destroy precondition), set the cluster's terminal
//!    status.

use std::sync::Arc;

use kubinate_addons::{model::AddonStatus, repository::AddonRepository};
use kubinate_cluster::{
    model::{ClusterNodeRole, ClusterStatus},
    repository::ClusterRepository,
    service::ClusterService,
};
use kubinate_integrations::{
    helm::{HelmExecutor, InstallParams},
    hetzner::HetznerProvider,
    kubectl::KubectlExecutor,
    ssh::{SshExecutor, SshTarget},
};
use kubinate_platform::audit::AuditContext;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use uuid::Uuid;

use crate::events::{ClusterEvent, ClusterEventHub};
use crate::workflows::{
    destroy_cluster, install_addon, provision_cluster, scale_in, scale_out, DestroyClusterInput,
    DestroyDeps, InstallAddonDeps, InstallAddonInput, NoopProgress, ProgressSink,
    ProvisionClusterInput, ProvisionDeps, ScaleInDeps, ScaleInInput, ScaleOutInput, WorkerToRemove,
};

/// Wires the workflow function to live infrastructure: the cluster
/// service (for status + kubeconfig), the cluster repo (for the
/// per-server inventory + shadow table), and the SSH executor.
///
/// The Hetzner provider is supplied per-call rather than stored on
/// the runner because each cluster uses its own tenant credential —
/// the API caller resolves the credential and assembles a
/// `kubinate_integrations::hetzner::Client` before invoking the
/// runner.
#[derive(Clone)]
pub struct LocalRunner {
    pool: PgPool,
    cluster_service: Arc<ClusterService>,
    cluster_repo: Arc<dyn ClusterRepository>,
    addon_repo: Arc<dyn AddonRepository>,
    ssh: Arc<dyn SshExecutor>,
    helm: Arc<dyn HelmExecutor>,
    kubectl: Arc<dyn KubectlExecutor>,
    /// Per-cluster event hub. The runner publishes step + terminal
    /// events; the API edge (`/v1/clusters/:id/events`) subscribes.
    /// See `crate::events`.
    events: Arc<ClusterEventHub>,
}

impl LocalRunner {
    /// Build a runner.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        pool: PgPool,
        cluster_service: Arc<ClusterService>,
        cluster_repo: Arc<dyn ClusterRepository>,
        addon_repo: Arc<dyn AddonRepository>,
        ssh: Arc<dyn SshExecutor>,
        helm: Arc<dyn HelmExecutor>,
        kubectl: Arc<dyn KubectlExecutor>,
        events: Arc<ClusterEventHub>,
    ) -> Self {
        Self {
            pool,
            cluster_service,
            cluster_repo,
            addon_repo,
            ssh,
            helm,
            kubectl,
            events,
        }
    }

    /// Hub handle for the API edge. The SSE handler subscribes
    /// against this; do not call `publish` from outside the runner.
    #[must_use]
    pub fn events(&self) -> Arc<ClusterEventHub> {
        self.events.clone()
    }

    /// Push a terminal event to SSE subscribers. Maps the cluster
    /// status to the wire-format string and pre-categorises the
    /// failure reason so the frontend never sees a raw activity
    /// error message.
    fn publish_terminal(&self, cluster_id: Uuid, status: ClusterStatus, reason: Option<&str>) {
        let status_str = match status {
            ClusterStatus::Ready => "ready",
            ClusterStatus::Failed => "failed",
            ClusterStatus::Destroyed => "destroyed",
            // Non-terminal states should never reach this helper —
            // callers only invoke after the runner has settled. Drop
            // the publish rather than silently mislabelling.
            _ => return,
        };
        let category = if matches!(status, ClusterStatus::Failed) {
            kubinate_cluster::status::error_category(status, reason).map(|c| c.as_str().to_string())
        } else {
            None
        };
        self.events.publish(
            cluster_id,
            ClusterEvent::Terminal {
                status: status_str.to_string(),
                error_category: category,
            },
        );
    }

    /// Synchronously execute a provisioning run. Use when the caller
    /// wants to await the result (e.g. the nightly E2E harness). The
    /// API edge prefers the spawn variant.
    ///
    /// # Errors
    /// Returns an error if provisioning fails at any activity step or if
    /// the database queries fail.
    pub async fn run_provision(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        input: ProvisionClusterInput,
    ) -> Result<(), anyhow::Error> {
        // Background-task audit context: no actor, but the workflow id
        // doubles as the request_id so an operator can stitch the
        // audit chain to a single workflow run.
        let workflow_row = Uuid::now_v7();
        let audit = AuditContext {
            actor_user_id: None,
            request_id: Some(workflow_row.to_string()),
            ip: None,
            user_agent: Some("runner/provision".to_string()),
        };

        self.cluster_repo
            .update_status(
                organization_id,
                input.cluster_id,
                ClusterStatus::Provisioning,
                None,
                &audit,
            )
            .await?;

        // Open the workflow shadow row up front so the UI sees a
        // current_step the moment polling kicks in.
        insert_workflow_row(&self.pool, workflow_row, organization_id, input.cluster_id).await?;

        let progress = Arc::new(WorkflowProgress {
            pool: self.pool.clone(),
            organization_id,
            workflow_id: workflow_row,
            cluster_id: input.cluster_id,
            events: self.events.clone(),
        });
        let deps = ProvisionDeps {
            hetzner: hetzner.clone(),
            ssh: self.ssh.clone(),
            progress: progress.clone(),
        };

        match provision_cluster(&deps, input.clone()).await {
            Ok(output) => {
                self.cluster_repo
                    .record_server(
                        organization_id,
                        input.cluster_id,
                        i64::try_from(output.control_plane.id.0).unwrap_or(i64::MAX),
                        ClusterNodeRole::ControlPlane,
                        output.control_plane.ipv4.as_deref(),
                        output.control_plane.private_ipv4.as_deref(),
                        &audit,
                    )
                    .await?;
                for w in &output.workers {
                    self.cluster_repo
                        .record_server(
                            organization_id,
                            input.cluster_id,
                            i64::try_from(w.id.0).unwrap_or(i64::MAX),
                            ClusterNodeRole::Worker,
                            w.ipv4.as_deref(),
                            w.private_ipv4.as_deref(),
                            &audit,
                        )
                        .await?;
                }
                self.cluster_service
                    .store_kubeconfig(
                        organization_id,
                        input.cluster_id,
                        SecretString::from(output.kubeconfig),
                        &audit,
                    )
                    .await?;
                self.cluster_repo
                    .update_status(
                        organization_id,
                        input.cluster_id,
                        ClusterStatus::Ready,
                        None,
                        &audit,
                    )
                    .await?;
                finalize_workflow_row(&self.pool, workflow_row, "done", None).await?;
                self.publish_terminal(input.cluster_id, ClusterStatus::Ready, None);
                Ok(())
            }
            Err(err) => {
                let reason = format_activity_reason(&err);
                self.cluster_repo
                    .update_status(
                        organization_id,
                        input.cluster_id,
                        ClusterStatus::Failed,
                        Some(&reason),
                        &audit,
                    )
                    .await
                    .ok();
                finalize_workflow_row(&self.pool, workflow_row, "failed", Some(&reason))
                    .await
                    .ok();
                self.publish_terminal(input.cluster_id, ClusterStatus::Failed, Some(&reason));
                Err(anyhow::anyhow!("provisioning failed: {reason}"))
            }
        }
    }

    /// Spawn a provisioning run on the runtime. Errors logged and
    /// reflected in the cluster row; the caller does not block.
    pub fn spawn_provision(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        input: ProvisionClusterInput,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let _g = WorkflowInflightGuard::new("provision");
            if let Err(err) = me.run_provision(organization_id, hetzner, input).await {
                tracing::error!(error = %err, "provisioning run errored");
            }
        });
    }

    /// Synchronously execute a destroy run. Idempotent against
    /// partially-destroyed clusters (ticket 05 ACs).
    ///
    /// # Errors
    /// Returns an error if the destroy workflow fails or if the database
    /// queries fail.
    pub async fn run_destroy(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        cluster_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        let workflow_row = Uuid::now_v7();
        let audit = AuditContext {
            actor_user_id: None,
            request_id: Some(workflow_row.to_string()),
            ip: None,
            user_agent: Some("runner/destroy".to_string()),
        };

        // begin_destroy flips status → destroying and rejects rerun on
        // already-destroyed clusters.
        self.cluster_service
            .begin_destroy(organization_id, cluster_id, &audit)
            .await?;

        let servers = self
            .cluster_repo
            .list_servers(organization_id, cluster_id)
            .await?;
        let server_ids = servers
            .iter()
            .map(|s| {
                kubinate_integrations::hetzner::ServerId(
                    u64::try_from(s.hetzner_server_id).unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();

        let progress = Arc::new(HubOnlyProgress {
            cluster_id,
            events: self.events.clone(),
        });
        let deps = DestroyDeps {
            hetzner: hetzner.clone(),
            progress,
        };

        if let Err(err) = destroy_cluster(
            &deps,
            DestroyClusterInput {
                cluster_id,
                server_ids,
            },
        )
        .await
        {
            let reason = format_activity_reason(&err);
            self.cluster_service
                .mark_destroy_failed(organization_id, cluster_id, &reason, &audit)
                .await
                .ok();
            self.publish_terminal(cluster_id, ClusterStatus::Failed, Some(&reason));
            return Err(anyhow::anyhow!("destroy failed: {reason}"));
        }

        // Record-side cleanup happens only after Hetzner confirmed
        // deletion: zero out cluster_servers, drop the kubeconfig
        // secret, soft-delete the cluster row.
        self.cluster_repo
            .soft_delete_servers(organization_id, cluster_id, &audit)
            .await?;
        self.cluster_service
            .finalize_destroy(organization_id, cluster_id, &audit)
            .await?;
        self.publish_terminal(cluster_id, ClusterStatus::Destroyed, None);
        Ok(())
    }

    /// Spawn a destroy run on the runtime; mirrors `spawn_provision`
    /// so the API handler can drop its bare `tokio::spawn` and the
    /// in-flight gauge stays consistent.
    pub fn spawn_destroy(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        cluster_id: Uuid,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let _g = WorkflowInflightGuard::new("destroy");
            if let Err(err) = me.run_destroy(organization_id, hetzner, cluster_id).await {
                tracing::error!(error = %err, cluster_id = %cluster_id, "destroy run errored");
            }
        });
    }

    /// Synchronously run the addon install workflow and mirror status
    /// into `cluster_addons`. Idempotent on the Helm side; the caller
    /// is responsible for resolving the cluster's kubeconfig path
    /// (the API materialises it from the secret store right before
    /// invoking).
    ///
    /// # Errors
    /// Returns an error if the Helm install fails or if the database
    /// queries fail.
    pub async fn run_install_addon(
        &self,
        organization_id: Uuid,
        cluster_addon_id: Uuid,
        params: InstallParams,
        kubeconfig_path: std::path::PathBuf,
    ) -> Result<(), anyhow::Error> {
        let audit = AuditContext {
            actor_user_id: None,
            request_id: Some(Uuid::now_v7().to_string()),
            ip: None,
            user_agent: Some("runner/install-addon".to_string()),
        };

        self.addon_repo
            .update_status(
                organization_id,
                cluster_addon_id,
                AddonStatus::Installing,
                None,
                &audit,
            )
            .await?;

        // Addon install does not transition the cluster's lifecycle
        // status; the per-addon UI still polls `/v1/clusters/:id/addons`.
        // Skip publishing to the cluster event hub.
        let progress = Arc::new(NoopProgress);
        let deps = InstallAddonDeps {
            helm: self.helm.clone(),
            progress,
        };

        let result = install_addon(
            &deps,
            InstallAddonInput {
                params,
                kubeconfig_path: kubeconfig_path.clone(),
            },
        )
        .await;

        // Best-effort cleanup of the kubeconfig tempfile, regardless
        // of outcome. The plaintext shouldn't outlive the install.
        let _ = tokio::fs::remove_file(&kubeconfig_path).await;

        match result {
            Ok(()) => {
                self.addon_repo
                    .update_status(
                        organization_id,
                        cluster_addon_id,
                        AddonStatus::Ready,
                        None,
                        &audit,
                    )
                    .await?;
                Ok(())
            }
            Err(err) => {
                let reason = format_activity_reason(&err);
                self.addon_repo
                    .update_status(
                        organization_id,
                        cluster_addon_id,
                        AddonStatus::Failed,
                        Some(&reason),
                        &audit,
                    )
                    .await
                    .ok();
                Err(anyhow::anyhow!("addon install failed: {reason}"))
            }
        }
    }

    /// Spawn an addon install on the runtime; errors are logged and
    /// reflected in the addon row.
    pub fn spawn_install_addon(
        &self,
        organization_id: Uuid,
        cluster_addon_id: Uuid,
        params: InstallParams,
        kubeconfig_path: std::path::PathBuf,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let _g = WorkflowInflightGuard::new("install_addon");
            if let Err(err) = me
                .run_install_addon(organization_id, cluster_addon_id, params, kubeconfig_path)
                .await
            {
                tracing::error!(error = %err, "addon install run errored");
            }
        });
    }

    /// Run an addon uninstall synchronously. Status transitions:
    /// `uninstalling → uninstalled` on success, `failed` on error.
    ///
    /// # Errors
    /// Returns an error if Helm fails or if the database status update fails.
    pub async fn run_uninstall_addon(
        &self,
        organization_id: Uuid,
        cluster_addon_id: Uuid,
        helm_release: String,
        namespace: String,
        kubeconfig_path: std::path::PathBuf,
    ) -> Result<(), anyhow::Error> {
        let audit = AuditContext {
            actor_user_id: None,
            request_id: Some(Uuid::now_v7().to_string()),
            ip: None,
            user_agent: Some("runner/uninstall-addon".to_string()),
        };

        let result = self
            .helm
            .uninstall(&kubeconfig_path, &helm_release, &namespace)
            .await;

        let _ = tokio::fs::remove_file(&kubeconfig_path).await;

        match result {
            Ok(()) => {
                self.addon_repo
                    .update_status(
                        organization_id,
                        cluster_addon_id,
                        AddonStatus::Uninstalled,
                        None,
                        &audit,
                    )
                    .await?;
                Ok(())
            }
            Err(err) => {
                let reason = err.to_string();
                self.addon_repo
                    .update_status(
                        organization_id,
                        cluster_addon_id,
                        AddonStatus::Failed,
                        Some(&reason),
                        &audit,
                    )
                    .await
                    .ok();
                Err(anyhow::anyhow!("addon uninstall failed: {reason}"))
            }
        }
    }

    /// Spawn an addon uninstall on the runtime; errors are logged and
    /// reflected in the addon row.
    pub fn spawn_uninstall_addon(
        &self,
        organization_id: Uuid,
        cluster_addon_id: Uuid,
        helm_release: String,
        namespace: String,
        kubeconfig_path: std::path::PathBuf,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let _g = WorkflowInflightGuard::new("uninstall_addon");
            if let Err(err) = me
                .run_uninstall_addon(
                    organization_id,
                    cluster_addon_id,
                    helm_release,
                    namespace,
                    kubeconfig_path,
                )
                .await
            {
                tracing::error!(error = %err, "addon uninstall run errored");
            }
        });
    }

    /// Sprint 3 ticket 08. Scale-out: add `count` workers to a Ready
    /// cluster. Caller is the API handler; resolution of CP endpoint
    /// + join token + `start_index` happens here.
    ///
    /// # Errors
    /// Returns an error if the scale-out workflow fails or if the database
    /// queries fail.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    pub async fn run_scale_out(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        cluster_id: Uuid,
        count: usize,
        cluster_name: String,
        location: String,
        server_type: String,
        ssh_key: String,
        user_data: String,
        k3s_version: String,
    ) -> Result<(), anyhow::Error> {
        let workflow_row = Uuid::now_v7();
        let audit = AuditContext {
            actor_user_id: None,
            request_id: Some(workflow_row.to_string()),
            ip: None,
            user_agent: Some("runner/scale-out".to_string()),
        };

        self.cluster_repo
            .update_status(
                organization_id,
                cluster_id,
                ClusterStatus::Scaling,
                None,
                &audit,
            )
            .await?;

        // Resolve existing servers: control plane → join target,
        // workers → next free index.
        let servers = self
            .cluster_repo
            .list_servers(organization_id, cluster_id)
            .await?;
        let cp = servers
            .iter()
            .find(|s| matches!(s.role, ClusterNodeRole::ControlPlane))
            .ok_or_else(|| anyhow::anyhow!("cluster has no control-plane server recorded"))?;
        let cp_ipv4 = cp
            .public_ipv4
            .clone()
            .ok_or_else(|| anyhow::anyhow!("control-plane row has no public IPv4"))?;
        let next_worker_index = servers
            .iter()
            .filter(|s| matches!(s.role, ClusterNodeRole::Worker))
            .count();

        // Read the k3s join token off the existing CP. Done here
        // (not as a workflow activity) so the workflow function stays
        // deterministic — it takes the token as input.
        let join_target = SshTarget {
            host: cp_ipv4.clone(),
            user: "kubinate".to_string(),
        };
        let join_token = self
            .ssh
            .run(
                &join_target,
                "sudo cat /var/lib/rancher/k3s/server/node-token",
            )
            .await
            .map_err(|e| anyhow::anyhow!("read join token: {e}"))?;
        let join_token = join_token.trim().to_string();

        let progress = Arc::new(HubOnlyProgress {
            cluster_id,
            events: self.events.clone(),
        });
        let deps = ProvisionDeps {
            hetzner: hetzner.clone(),
            ssh: self.ssh.clone(),
            progress,
        };

        let result = scale_out(
            &deps,
            ScaleOutInput {
                cluster_id,
                cluster_name,
                location,
                server_type,
                ssh_key,
                user_data,
                k3s_version,
                control_plane_endpoint: cp_ipv4,
                join_token,
                start_index: next_worker_index,
                count,
            },
        )
        .await;

        match result {
            Ok(out) => {
                for w in &out.workers {
                    self.cluster_repo
                        .record_server(
                            organization_id,
                            cluster_id,
                            i64::try_from(w.id.0).unwrap_or(i64::MAX),
                            ClusterNodeRole::Worker,
                            w.ipv4.as_deref(),
                            w.private_ipv4.as_deref(),
                            &audit,
                        )
                        .await?;
                }
                self.cluster_repo
                    .update_status(
                        organization_id,
                        cluster_id,
                        ClusterStatus::Ready,
                        None,
                        &audit,
                    )
                    .await?;
                self.publish_terminal(cluster_id, ClusterStatus::Ready, None);
                Ok(())
            }
            Err(err) => {
                let reason = format_activity_reason(&err);
                self.cluster_repo
                    .update_status(
                        organization_id,
                        cluster_id,
                        ClusterStatus::Failed,
                        Some(&reason),
                        &audit,
                    )
                    .await
                    .ok();
                self.publish_terminal(cluster_id, ClusterStatus::Failed, Some(&reason));
                Err(anyhow::anyhow!("scale-out failed: {reason}"))
            }
        }
    }

    /// Sprint 3 ticket 08. Scale-in: drain → kubectl-delete → Hetzner-
    /// delete the requested workers. The runner picks the youngest
    /// `count` workers (highest index first) to keep node names
    /// stable across operations.
    ///
    /// # Errors
    /// Returns an error if the scale-in workflow fails or if the database
    /// queries fail.
    #[allow(clippy::too_many_lines)]
    pub async fn run_scale_in(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        cluster_id: Uuid,
        cluster_name: String,
        count: usize,
    ) -> Result<(), anyhow::Error> {
        let workflow_row = Uuid::now_v7();
        let audit = AuditContext {
            actor_user_id: None,
            request_id: Some(workflow_row.to_string()),
            ip: None,
            user_agent: Some("runner/scale-in".to_string()),
        };

        self.cluster_repo
            .update_status(
                organization_id,
                cluster_id,
                ClusterStatus::Scaling,
                None,
                &audit,
            )
            .await?;

        let servers = self
            .cluster_repo
            .list_servers(organization_id, cluster_id)
            .await?;
        let mut workers: Vec<_> = servers
            .iter()
            .filter(|s| matches!(s.role, ClusterNodeRole::Worker))
            .collect();
        // Highest-index-first: a stable mapping between
        // `worker_params` index and Hetzner hostname (`<cluster>-worker-NN`)
        // means the youngest workers are the safest to drop.
        workers.sort_by(|a, b| b.hetzner_server_id.cmp(&a.hetzner_server_id));
        let to_remove: Vec<_> = workers.into_iter().take(count).collect();

        // Build k8s node names from the cluster + index. The
        // hostname pattern is `<cluster>-worker-NN`; we don't have
        // the index column on `cluster_servers` so reconstruct from
        // sorted order: the lowest server-id worker is index 00, etc.
        // For the typical case where workers were appended in order
        // this is exact; in pathological cases a follow-up ticket
        // adds an explicit `node_name` column.
        let mut all_workers: Vec<_> = servers
            .iter()
            .filter(|s| matches!(s.role, ClusterNodeRole::Worker))
            .collect();
        all_workers.sort_by_key(|s| s.hetzner_server_id);
        let index_for_id: std::collections::HashMap<i64, usize> = all_workers
            .iter()
            .enumerate()
            .map(|(i, s)| (s.hetzner_server_id, i))
            .collect();

        let targets: Vec<WorkerToRemove> = to_remove
            .iter()
            .map(|s| {
                let idx = index_for_id.get(&s.hetzner_server_id).copied().unwrap_or(0);
                WorkerToRemove {
                    server_id: kubinate_integrations::hetzner::ServerId(
                        u64::try_from(s.hetzner_server_id).unwrap_or_default(),
                    ),
                    node_name: format!("{cluster_name}-worker-{idx:02}"),
                }
            })
            .collect();

        // Materialise the kubeconfig once — the kubectl wrapper reads
        // it from disk via KUBECONFIG env. Removed at the end no
        // matter what (mirrors the addon-install path).
        let kubeconfig = self
            .cluster_service
            .fetch_kubeconfig(organization_id, cluster_id)
            .await?;
        let kubeconfig_path =
            std::env::temp_dir().join(format!("kubinate-scale-in-{}.yaml", Uuid::now_v7()));
        tokio::fs::write(&kubeconfig_path, kubeconfig.expose_secret())
            .await
            .map_err(|e| anyhow::anyhow!("stage kubeconfig: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(
                &kubeconfig_path,
                std::fs::Permissions::from_mode(0o600),
            )
            .await;
        }

        let progress = Arc::new(HubOnlyProgress {
            cluster_id,
            events: self.events.clone(),
        });
        let deps = ScaleInDeps {
            hetzner: hetzner.clone(),
            kubectl: self.kubectl.clone(),
            progress,
        };

        let result = scale_in(
            &deps,
            ScaleInInput {
                cluster_id,
                targets,
                kubeconfig_path: kubeconfig_path.clone(),
            },
        )
        .await;

        let _ = tokio::fs::remove_file(&kubeconfig_path).await;

        match result {
            Ok(()) => {
                // Soft-delete the rows we removed.
                for s in &to_remove {
                    sqlx::query(
                        "UPDATE cluster_servers SET deleted_at = now()
                         WHERE id = $1 AND deleted_at IS NULL",
                    )
                    .bind(s.id)
                    .execute(&self.pool)
                    .await
                    .ok();
                }
                self.cluster_repo
                    .update_status(
                        organization_id,
                        cluster_id,
                        ClusterStatus::Ready,
                        None,
                        &audit,
                    )
                    .await?;
                self.publish_terminal(cluster_id, ClusterStatus::Ready, None);
                Ok(())
            }
            Err(err) => {
                let reason = format_activity_reason(&err);
                self.cluster_repo
                    .update_status(
                        organization_id,
                        cluster_id,
                        ClusterStatus::Failed,
                        Some(&reason),
                        &audit,
                    )
                    .await
                    .ok();
                self.publish_terminal(cluster_id, ClusterStatus::Failed, Some(&reason));
                Err(anyhow::anyhow!("scale-in failed: {reason}"))
            }
        }
    }

    /// Spawn a scale-out run. Errors logged + reflected in the
    /// cluster status; the caller does not block.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_scale_out(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        cluster_id: Uuid,
        count: usize,
        cluster_name: String,
        location: String,
        server_type: String,
        ssh_key: String,
        user_data: String,
        k3s_version: String,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let _g = WorkflowInflightGuard::new("scale_out");
            if let Err(err) = me
                .run_scale_out(
                    organization_id,
                    hetzner,
                    cluster_id,
                    count,
                    cluster_name,
                    location,
                    server_type,
                    ssh_key,
                    user_data,
                    k3s_version,
                )
                .await
            {
                tracing::error!(error = %err, "scale-out run errored");
            }
        });
    }

    /// Spawn a scale-in run.
    pub fn spawn_scale_in(
        &self,
        organization_id: Uuid,
        hetzner: Arc<dyn HetznerProvider>,
        cluster_id: Uuid,
        cluster_name: String,
        count: usize,
    ) {
        let me = self.clone();
        tokio::spawn(async move {
            let _g = WorkflowInflightGuard::new("scale_in");
            if let Err(err) = me
                .run_scale_in(organization_id, hetzner, cluster_id, cluster_name, count)
                .await
            {
                tracing::error!(error = %err, "scale-in run errored");
            }
        });
    }
}

/// Suppress unused-import warning when building without the addon
/// feature in the future. `ExposeSecret` is referenced when we
/// materialise a kubeconfig tempfile (currently in the API handler).
#[allow(dead_code)]
fn _force_expose_secret_in_use(s: &SecretString) {
    let _ = s.expose_secret();
}

async fn insert_workflow_row(
    pool: &PgPool,
    workflow_id: Uuid,
    organization_id: Uuid,
    cluster_id: Uuid,
) -> Result<(), anyhow::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(&format!(
        "SET LOCAL app.current_tenant_id = '{organization_id}'",
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO provisioning_workflows
            (id, cluster_id, organization_id, current_step)
         VALUES ($1, $2, $3, 'queued')",
    )
    .bind(workflow_id)
    .bind(cluster_id)
    .bind(organization_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn finalize_workflow_row(
    pool: &PgPool,
    workflow_id: Uuid,
    final_step: &str,
    error_reason: Option<&str>,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        r"
        UPDATE provisioning_workflows
        SET current_step = $2::provisioning_step,
            error_reason = $3,
            finished_at = now()
        WHERE id = $1
        ",
    )
    .bind(workflow_id)
    .bind(final_step)
    .bind(error_reason)
    .execute(pool)
    .await?;
    Ok(())
}

fn format_activity_reason(err: &crate::activities::ActivityError) -> String {
    use crate::activities::ActivityError;
    use kubinate_integrations::hetzner::HetznerError;
    use kubinate_integrations::ssh::SshError;
    match err {
        ActivityError::Hetzner(HetznerError::Status { status, .. }) => {
            format!("hetzner: {status}")
        }
        ActivityError::Hetzner(HetznerError::Transport(_)) => "hetzner: transport".to_string(),
        ActivityError::Hetzner(HetznerError::Credential(_)) => "hetzner: credential".to_string(),
        ActivityError::Ssh(SshError::NonZeroExit { code, .. }) => {
            format!("ssh: NonZeroExit code={code}")
        }
        ActivityError::Ssh(SshError::Connect(_)) => "ssh: connect".to_string(),
        ActivityError::Ssh(SshError::Transport(_)) => "ssh: transport".to_string(),
        ActivityError::CloudInitTimeout { attempts } => {
            format!("cloud-init timed out after {attempts} polls")
        }
        ActivityError::Helm(kubinate_integrations::helm::HelmError::NonZeroExit {
            code, ..
        }) => {
            format!("helm: NonZeroExit code={code}")
        }
        ActivityError::Helm(kubinate_integrations::helm::HelmError::Timeout(_)) => {
            "helm: timeout".to_string()
        }
        ActivityError::Helm(kubinate_integrations::helm::HelmError::Transport(_)) => {
            "helm: transport".to_string()
        }
        ActivityError::Kubectl(kubinate_integrations::kubectl::KubectlError::NonZeroExit {
            code,
            ..
        }) => format!("kubectl: NonZeroExit code={code}"),
        ActivityError::Kubectl(kubinate_integrations::kubectl::KubectlError::Timeout(_)) => {
            "kubectl: timeout".to_string()
        }
        ActivityError::Kubectl(kubinate_integrations::kubectl::KubectlError::Transport(_)) => {
            "kubectl: transport".to_string()
        }
        ActivityError::Other(_) => "other".to_string(),
    }
}

struct WorkflowProgress {
    pool: PgPool,
    organization_id: Uuid,
    workflow_id: Uuid,
    /// Cluster the workflow operates on. Used to fan out step
    /// transitions to SSE subscribers.
    cluster_id: Uuid,
    events: Arc<ClusterEventHub>,
}

#[async_trait::async_trait]
impl ProgressSink for WorkflowProgress {
    async fn step(&self, step: &str) {
        // Best-effort write; logging-only on failure since the
        // workflow proper is the source of truth.
        if let Err(err) =
            update_step(&self.pool, self.organization_id, self.workflow_id, step).await
        {
            tracing::warn!(error = %err, step = %step, "failed to mirror workflow step");
        }
        // Publish to any connected SSE subscribers. No-op when none —
        // the workflow keeps running normally.
        self.events.publish(
            self.cluster_id,
            ClusterEvent::Step {
                step: step.to_string(),
            },
        );
    }
}

async fn update_step(
    pool: &PgPool,
    organization_id: Uuid,
    workflow_id: Uuid,
    step: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(&format!(
        "SET LOCAL app.current_tenant_id = '{organization_id}'",
    ))
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE provisioning_workflows
         SET current_step = $2::provisioning_step
         WHERE id = $1",
    )
    .bind(workflow_id)
    .bind(step)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Destroy / scale flows don't yet have their own shadow-table
/// column scheme, but they still want SSE subscribers to see step
/// transitions. This sink only publishes to the hub.
struct HubOnlyProgress {
    cluster_id: Uuid,
    events: Arc<ClusterEventHub>,
}

#[async_trait::async_trait]
impl ProgressSink for HubOnlyProgress {
    async fn step(&self, step: &str) {
        self.events.publish(
            self.cluster_id,
            ClusterEvent::Step {
                step: step.to_string(),
            },
        );
    }
}

/// RAII guard that increments the
/// `kubinate_runner_workflows_inflight` gauge on construction and
/// decrements it on Drop. The label `kind` distinguishes which
/// flavour of workflow is running so an operator can tell whether
/// the pressure is from provisions, destroys, or scales.
///
/// Defer-path AC for Sprint 3 ticket 01: this gauge is the
/// operational signal that decides when we revisit the Temporal
/// SDK adoption decision (sustained > 10 means revisit per
/// `docs/decisions/sprint-2-temporal.md`).
struct WorkflowInflightGuard {
    kind: &'static str,
}

impl WorkflowInflightGuard {
    fn new(kind: &'static str) -> Self {
        metrics::gauge!(
            kubinate_platform::metrics::names::RUNNER_WORKFLOWS_INFLIGHT,
            "kind" => kind
        )
        .increment(1.0);
        Self { kind }
    }
}

impl Drop for WorkflowInflightGuard {
    fn drop(&mut self) {
        metrics::gauge!(
            kubinate_platform::metrics::names::RUNNER_WORKFLOWS_INFLIGHT,
            "kind" => self.kind
        )
        .decrement(1.0);
    }
}
