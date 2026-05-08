//! Workflow orchestration.
//!
//! This module composes the five activities from [`crate::activities`]
//! into one function. The function is Temporal-shape (deterministic,
//! side effects only through injected dependencies) so that when the
//! ADR-0003 SDK spike concludes, wiring it as a real Temporal workflow
//! is mostly a matter of wrapping calls in activity proxies.

use std::sync::Arc;

use kubinate_integrations::{
    helm::{HelmExecutor, InstallParams},
    hetzner::{HetznerProvider, HetznerServer, ServerId},
    kubectl::KubectlExecutor,
    ssh::{SshExecutor, SshTarget},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::activities::{
    collect_kubeconfig, control_plane_params, helm_install_chart, hetzner_create_server,
    hetzner_delete_server_idempotent, kubectl_delete_node, kubectl_drain_node,
    ssh_install_k3s_agent, ssh_install_k3s_server, wait_for_cloud_init, worker_params,
    ActivityError,
};

/// Input contract for [`provision_cluster`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionClusterInput {
    /// The cluster row this workflow is reconciling.
    pub cluster_id: Uuid,
    /// User-facing cluster name (used in Hetzner server hostnames).
    pub cluster_name: String,
    /// Hetzner location slug.
    pub location: String,
    /// Hetzner server type slug.
    pub server_type: String,
    /// Number of worker nodes to create.
    pub worker_count: usize,
    /// SSH public key fingerprint / id registered with the Hetzner
    /// project for this run.
    pub ssh_key: String,
    /// cloud-init document shipped to every node.
    pub user_data: String,
    /// k3s version to install (e.g. `v1.30.2+k3s1`).
    pub k3s_version: String,
}

/// Result of a successful run. Unsuccessful runs return
/// [`ActivityError`] — the runner is responsible for mapping that into
/// the cluster's `failed` status.
#[derive(Debug, Clone)]
pub struct ProvisionClusterOutput {
    /// Control-plane server view (id + IPs).
    pub control_plane: HetznerServer,
    /// Worker server views (order matches input indices).
    pub workers: Vec<HetznerServer>,
    /// Rewritten kubeconfig, ready for storage via `SecretStore::put`
    /// in ticket 04.
    pub kubeconfig: String,
}

/// Hooks the runner uses to mirror workflow progress into the shadow
/// table. Kept as a trait so tests can observe step transitions
/// without a live Postgres.
#[async_trait::async_trait]
pub trait ProgressSink: Send + Sync {
    /// Called when the workflow enters a new step. Implementations
    /// should be idempotent — the same step may be reported twice if
    /// the workflow is replayed.
    async fn step(&self, step: &str);
}

/// Dependencies the workflow draws on.
#[derive(Clone)]
pub struct ProvisionDeps {
    /// Hetzner API surface.
    pub hetzner: Arc<dyn HetznerProvider>,
    /// SSH executor.
    pub ssh: Arc<dyn SshExecutor>,
    /// Shadow-table progress sink.
    pub progress: Arc<dyn ProgressSink>,
}

/// Happy-path provisioning. Compensation (server delete on failure)
/// lives with ticket 05's `DestroyClusterWorkflow`.
pub async fn provision_cluster(
    deps: &ProvisionDeps,
    input: ProvisionClusterInput,
) -> Result<ProvisionClusterOutput, ActivityError> {
    // 1. Control plane.
    deps.progress.step("creating_control_plane").await;
    let cp_params = control_plane_params(
        input.cluster_id,
        &input.cluster_name,
        &input.server_type,
        &input.location,
        &input.ssh_key,
        &input.user_data,
    );
    let cp_id = hetzner_create_server(&*deps.hetzner, cp_params).await?;

    deps.progress.step("waiting_cloud_init_cp").await;
    let cp_server = wait_for_cloud_init(&*deps.hetzner, cp_id).await?;
    let cp_target = ssh_target(&cp_server)?;

    // 2. k3s server on the control plane.
    deps.progress.step("installing_k3s_server").await;
    let join_token =
        ssh_install_k3s_server(&*deps.ssh, cp_target.clone(), &input.k3s_version).await?;

    // 3. Workers.
    deps.progress.step("creating_workers").await;
    let mut worker_ids = Vec::with_capacity(input.worker_count);
    for index in 0..input.worker_count {
        let params = worker_params(
            input.cluster_id,
            &input.cluster_name,
            index,
            &input.server_type,
            &input.location,
            &input.ssh_key,
            &input.user_data,
        );
        worker_ids.push(hetzner_create_server(&*deps.hetzner, params).await?);
    }

    deps.progress.step("waiting_cloud_init_workers").await;
    let mut worker_servers = Vec::with_capacity(worker_ids.len());
    for id in &worker_ids {
        worker_servers.push(wait_for_cloud_init(&*deps.hetzner, *id).await?);
    }

    // 4. Join workers as agents.
    deps.progress.step("installing_k3s_agents").await;
    let cp_private = cp_server
        .private_ipv4
        .as_deref()
        .unwrap_or_else(|| cp_server.ipv4.as_deref().unwrap_or("0.0.0.0"));
    for worker in &worker_servers {
        let target = ssh_target(worker)?;
        ssh_install_k3s_agent(
            &*deps.ssh,
            target,
            cp_private,
            &join_token,
            &input.k3s_version,
        )
        .await?;
    }

    // 5. Kubeconfig.
    deps.progress.step("collecting_kubeconfig").await;
    let public_endpoint = format!(
        "https://{}:6443",
        cp_server.ipv4.as_deref().unwrap_or("0.0.0.0"),
    );
    let kubeconfig = collect_kubeconfig(&*deps.ssh, cp_target, &public_endpoint).await?;

    deps.progress.step("done").await;
    Ok(ProvisionClusterOutput {
        control_plane: cp_server,
        workers: worker_servers,
        kubeconfig,
    })
}

fn ssh_target(
    server: &kubinate_integrations::hetzner::HetznerServer,
) -> Result<SshTarget, ActivityError> {
    let host = server.ipv4.clone().ok_or_else(|| {
        ActivityError::Other(anyhow::anyhow!(
            "server {:?} reported Ready but has no IPv4",
            server.id
        ))
    })?;
    Ok(SshTarget {
        host,
        user: "kubinate".to_string(),
    })
}

/// Input contract for [`destroy_cluster`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DestroyClusterInput {
    /// The cluster row this workflow is reconciling.
    pub cluster_id: Uuid,
    /// Hetzner server ids previously created for this cluster. The
    /// caller is responsible for resolving these (in production, from
    /// the `cluster_servers` shadow table; in tests, directly).
    pub server_ids: Vec<ServerId>,
}

/// Dependencies the destroy workflow draws on. Subset of
/// [`ProvisionDeps`] — no SSH, no k3s install.
#[derive(Clone)]
pub struct DestroyDeps {
    /// Hetzner API surface.
    pub hetzner: Arc<dyn HetznerProvider>,
    /// Shadow-table progress sink.
    pub progress: Arc<dyn ProgressSink>,
}

/// Symmetric to [`provision_cluster`]: deletes every server we created
/// for the cluster, treating "already gone" as success so a re-run
/// after a partial destroy completes cleanly (ticket 05 AC).
///
/// Note: the workflow is *only* responsible for Hetzner-side cleanup.
/// The cluster row's soft-delete and the kubeconfig-secret deletion
/// happen in the orchestrating service (so a Temporal replay of just
/// the Hetzner-delete steps stays deterministic).
pub async fn destroy_cluster(
    deps: &DestroyDeps,
    input: DestroyClusterInput,
) -> Result<(), ActivityError> {
    deps.progress.step("destroying_servers").await;
    for id in &input.server_ids {
        hetzner_delete_server_idempotent(&*deps.hetzner, *id).await?;
    }
    deps.progress.step("done").await;
    Ok(())
}

/// Input contract for [`install_addon`].
#[derive(Debug, Clone)]
pub struct InstallAddonInput {
    /// Helm install parameters resolved from the addon catalog.
    pub params: InstallParams,
    /// Path to the cluster's kubeconfig on the runner host.
    pub kubeconfig_path: std::path::PathBuf,
}

/// Dependencies the addon-install workflow draws on.
#[derive(Clone)]
pub struct InstallAddonDeps {
    /// Helm CLI wrapper.
    pub helm: Arc<dyn HelmExecutor>,
    /// Shadow-table progress sink.
    pub progress: Arc<dyn ProgressSink>,
}

/// Sprint 2 ticket 07. Linear `helm upgrade --install` against the
/// cluster's kubeconfig. Idempotent at the Helm level — re-runs
/// converge to the requested state.
pub async fn install_addon(
    deps: &InstallAddonDeps,
    input: InstallAddonInput,
) -> Result<(), ActivityError> {
    deps.progress.step("installing").await;
    helm_install_chart(&*deps.helm, &input.kubeconfig_path, &input.params).await?;
    deps.progress.step("done").await;
    Ok(())
}

/// Input contract for [`scale_out`] — add `count` workers starting
/// at `start_index`. Caller (the runner) computes `start_index` by
/// `MAX(existing worker index) + 1` so worker hostnames don't reuse.
#[derive(Debug, Clone)]
pub struct ScaleOutInput {
    /// Cluster being scaled.
    pub cluster_id: Uuid,
    /// User-facing cluster name (used in Hetzner server hostnames).
    pub cluster_name: String,
    /// Hetzner location slug.
    pub location: String,
    /// Hetzner server type slug.
    pub server_type: String,
    /// SSH key reference for the new servers.
    pub ssh_key: String,
    /// cloud-init document.
    pub user_data: String,
    /// k3s version (must match the existing CP).
    pub k3s_version: String,
    /// Public IPv4 of an existing CP, used as the join target.
    pub control_plane_endpoint: String,
    /// Existing CP join token. The runner reads this off-cluster via
    /// SSH; passing in lets workflow tests stay deterministic.
    pub join_token: String,
    /// First worker index to allocate.
    pub start_index: usize,
    /// How many workers to add.
    pub count: usize,
}

/// Result of a successful scale-out.
#[derive(Debug, Clone)]
pub struct ScaleOutOutput {
    /// Server views for every worker added (id + IPs). Order matches
    /// indices `start_index..start_index+count`.
    pub workers: Vec<HetznerServer>,
}

/// Add N worker nodes to an existing cluster. Sequential to keep
/// the join path serialised (k3s agent join is sensitive to control-
/// plane load).
pub async fn scale_out(
    deps: &ProvisionDeps,
    input: ScaleOutInput,
) -> Result<ScaleOutOutput, ActivityError> {
    deps.progress.step("creating_workers").await;

    let mut ids = Vec::with_capacity(input.count);
    for offset in 0..input.count {
        let index = input.start_index + offset;
        let params = worker_params(
            input.cluster_id,
            &input.cluster_name,
            index,
            &input.server_type,
            &input.location,
            &input.ssh_key,
            &input.user_data,
        );
        ids.push(hetzner_create_server(&*deps.hetzner, params).await?);
    }

    deps.progress.step("waiting_cloud_init_workers").await;
    let mut workers = Vec::with_capacity(ids.len());
    for id in &ids {
        workers.push(wait_for_cloud_init(&*deps.hetzner, *id).await?);
    }

    deps.progress.step("installing_k3s_agents").await;
    for w in &workers {
        let target = ssh_target(w)?;
        ssh_install_k3s_agent(
            &*deps.ssh,
            target,
            &input.control_plane_endpoint,
            &input.join_token,
            &input.k3s_version,
        )
        .await?;
    }

    deps.progress.step("done").await;
    Ok(ScaleOutOutput { workers })
}

/// One worker selected for removal during scale-in.
#[derive(Debug, Clone)]
pub struct WorkerToRemove {
    /// Hetzner server id.
    pub server_id: ServerId,
    /// k8s node name (matches the Hetzner server hostname; the runner
    /// resolves it from the `cluster_servers` row).
    pub node_name: String,
}

/// Input for [`scale_in`].
#[derive(Debug, Clone)]
pub struct ScaleInInput {
    /// Cluster being scaled.
    pub cluster_id: Uuid,
    /// Workers to drain + delete.
    pub targets: Vec<WorkerToRemove>,
    /// Path to the cluster's kubeconfig on the runner host (for the
    /// drain step). The runner materialises this as a 0600 tempfile
    /// and removes it after the workflow returns.
    pub kubeconfig_path: std::path::PathBuf,
}

/// Dependencies for the scale-in flow. Adds the kubectl executor on
/// top of the standard provision deps.
#[derive(Clone)]
pub struct ScaleInDeps {
    /// Hetzner API surface (for the server delete).
    pub hetzner: std::sync::Arc<dyn HetznerProvider>,
    /// kubectl wrapper (drain + delete-node).
    pub kubectl: std::sync::Arc<dyn KubectlExecutor>,
    /// Progress sink.
    pub progress: std::sync::Arc<dyn ProgressSink>,
}

/// Drain → kubectl-delete → Hetzner-delete each target. Idempotent
/// against re-runs: the kubectl wrapper treats `NotFound` as success
/// and `hetzner_delete_server_idempotent` swallows 404.
pub async fn scale_in(deps: &ScaleInDeps, input: ScaleInInput) -> Result<(), ActivityError> {
    deps.progress.step("draining_nodes").await;
    for t in &input.targets {
        kubectl_drain_node(&*deps.kubectl, &input.kubeconfig_path, &t.node_name).await?;
    }

    deps.progress.step("deleting_nodes").await;
    for t in &input.targets {
        kubectl_delete_node(&*deps.kubectl, &input.kubeconfig_path, &t.node_name).await?;
    }

    deps.progress.step("destroying_servers").await;
    for t in &input.targets {
        hetzner_delete_server_idempotent(&*deps.hetzner, t.server_id).await?;
    }

    deps.progress.step("done").await;
    Ok(())
}

/// Empty `ProgressSink` — handy for tests and anywhere the workflow
/// runs without a shadow-table write path.
pub struct NoopProgress;

#[async_trait::async_trait]
impl ProgressSink for NoopProgress {
    async fn step(&self, _step: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kubinate_integrations::helm::HelmError;
    use kubinate_integrations::hetzner::{
        CreateServerParams, HetznerError, HetznerServer, ServerStatus,
    };
    use kubinate_integrations::ssh::SshError;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    struct FakeHetzner {
        next: AtomicUsize,
        created: Mutex<Vec<CreateServerParams>>,
    }
    #[async_trait]
    impl HetznerProvider for FakeHetzner {
        async fn create_server(
            &self,
            params: CreateServerParams,
        ) -> Result<ServerId, HetznerError> {
            let id = ServerId(self.next.fetch_add(1, Ordering::SeqCst) as u64);
            self.created.lock().unwrap().push(params);
            Ok(id)
        }
        async fn get_server(&self, id: ServerId) -> Result<HetznerServer, HetznerError> {
            Ok(HetznerServer {
                id,
                status: ServerStatus::Ready,
                ipv4: Some(format!("203.0.113.{}", id.0 % 250)),
                private_ipv4: Some(format!("10.20.1.{}", id.0 % 250)),
            })
        }
        async fn delete_server(&self, _id: ServerId) -> Result<(), HetznerError> {
            Ok(())
        }
    }

    struct FakeSsh;
    #[async_trait]
    impl SshExecutor for FakeSsh {
        async fn run(&self, _target: &SshTarget, command: &str) -> Result<String, SshError> {
            if command.starts_with("sudo cat /var/lib/rancher/k3s") {
                return Ok("K10test-token::server".into());
            }
            if command.starts_with("sudo cat /etc/rancher/k3s") {
                return Ok(
                    "apiVersion: v1\nserver: https://127.0.0.1:6443\ncluster: test\n".into(),
                );
            }
            Ok(String::new())
        }
    }

    struct RecordingProgress {
        steps: Mutex<Vec<String>>,
    }
    #[async_trait]
    impl ProgressSink for RecordingProgress {
        async fn step(&self, step: &str) {
            self.steps.lock().unwrap().push(step.to_string());
        }
    }

    fn fixed_input(cluster_id: Uuid) -> ProvisionClusterInput {
        ProvisionClusterInput {
            cluster_id,
            cluster_name: "acme-prod".into(),
            location: "nbg1".into(),
            server_type: "cpx21".into(),
            worker_count: 2,
            ssh_key: "ssh-ed25519 AAAA...".into(),
            user_data: "#cloud-config\n".into(),
            k3s_version: "v1.30.2+k3s1".into(),
        }
    }

    /// Stand-in for Temporal's replay test: run the same workflow twice
    /// against fresh fake dependencies and assert the workflow makes
    /// exactly the same Hetzner calls and emits exactly the same step
    /// transitions. The real Temporal replay verifier compares the
    /// recorded history against a re-execution; this is the same idea
    /// at a coarser granularity.
    #[tokio::test]
    async fn workflow_is_deterministic_under_replay() {
        async fn run() -> (Vec<String>, Vec<String>) {
            let hetzner = Arc::new(FakeHetzner {
                next: AtomicUsize::new(1000),
                created: Mutex::new(Vec::new()),
            });
            let ssh = Arc::new(FakeSsh);
            let progress = Arc::new(RecordingProgress {
                steps: Mutex::new(Vec::new()),
            });
            let deps = ProvisionDeps {
                hetzner: hetzner.clone(),
                ssh,
                progress: progress.clone(),
            };
            // Pinned cluster id keeps idempotency_key deterministic
            // across runs — exactly what Temporal's replay relies on.
            provision_cluster(&deps, fixed_input(Uuid::nil()))
                .await
                .unwrap();

            let names = hetzner
                .created
                .lock()
                .unwrap()
                .iter()
                .map(|p| format!("{}|{}", p.name, p.idempotency_key))
                .collect();
            let steps = progress.steps.lock().unwrap().clone();
            (names, steps)
        }

        let first = run().await;
        let second = run().await;
        assert_eq!(first, second, "workflow must be deterministic under replay");
    }

    /// Variant of `FakeHetzner` whose `delete_server` impl is scripted:
    /// callers register which ids must respond `404` to simulate a
    /// previously-partial destroy. Created with a separate type so the
    /// happy-path provision tests don't have to reason about delete state.
    struct DestructibleHetzner {
        deleted: Mutex<Vec<ServerId>>,
        already_gone: std::collections::HashSet<u64>,
    }
    impl DestructibleHetzner {
        fn new(already_gone: &[u64]) -> Self {
            Self {
                deleted: Mutex::new(Vec::new()),
                already_gone: already_gone.iter().copied().collect(),
            }
        }
    }
    #[async_trait]
    impl HetznerProvider for DestructibleHetzner {
        async fn create_server(&self, _: CreateServerParams) -> Result<ServerId, HetznerError> {
            unreachable!("destroy workflow does not create servers")
        }
        async fn get_server(&self, _: ServerId) -> Result<HetznerServer, HetznerError> {
            unreachable!("destroy workflow does not poll servers")
        }
        async fn delete_server(&self, id: ServerId) -> Result<(), HetznerError> {
            if self.already_gone.contains(&id.0) {
                return Err(HetznerError::Status {
                    status: 404,
                    body: "not found".into(),
                });
            }
            self.deleted.lock().unwrap().push(id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn destroy_deletes_every_server_in_input() {
        let hetzner = Arc::new(DestructibleHetzner::new(&[]));
        let progress = Arc::new(RecordingProgress {
            steps: Mutex::new(Vec::new()),
        });
        let deps = DestroyDeps {
            hetzner: hetzner.clone(),
            progress: progress.clone(),
        };
        destroy_cluster(
            &deps,
            DestroyClusterInput {
                cluster_id: Uuid::now_v7(),
                server_ids: vec![ServerId(101), ServerId(102), ServerId(103)],
            },
        )
        .await
        .unwrap();
        assert_eq!(hetzner.deleted.lock().unwrap().len(), 3);
        assert_eq!(
            progress.steps.lock().unwrap().clone(),
            vec!["destroying_servers", "done"],
        );
    }

    #[tokio::test]
    async fn destroy_is_idempotent_when_some_servers_already_gone() {
        let hetzner = Arc::new(DestructibleHetzner::new(&[101, 103]));
        let deps = DestroyDeps {
            hetzner: hetzner.clone(),
            progress: Arc::new(NoopProgress),
        };
        destroy_cluster(
            &deps,
            DestroyClusterInput {
                cluster_id: Uuid::now_v7(),
                server_ids: vec![ServerId(101), ServerId(102), ServerId(103)],
            },
        )
        .await
        .expect("partial state should still complete");
        // Only 102 was actually deleted; 101 and 103 already-gone returned 404.
        assert_eq!(hetzner.deleted.lock().unwrap().clone(), vec![ServerId(102)],);
    }

    #[tokio::test]
    async fn destroy_succeeds_when_everything_already_gone() {
        let hetzner = Arc::new(DestructibleHetzner::new(&[201, 202]));
        let deps = DestroyDeps {
            hetzner: hetzner.clone(),
            progress: Arc::new(NoopProgress),
        };
        destroy_cluster(
            &deps,
            DestroyClusterInput {
                cluster_id: Uuid::now_v7(),
                server_ids: vec![ServerId(201), ServerId(202)],
            },
        )
        .await
        .expect("re-run on fully destroyed cluster should succeed");
        assert!(hetzner.deleted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn happy_path_runs_all_steps_in_order() {
        let hetzner = Arc::new(FakeHetzner {
            next: AtomicUsize::new(1000),
            created: Mutex::new(Vec::new()),
        });
        let ssh = Arc::new(FakeSsh);
        let progress = Arc::new(RecordingProgress {
            steps: Mutex::new(Vec::new()),
        });

        let deps = ProvisionDeps {
            hetzner: hetzner.clone(),
            ssh,
            progress: progress.clone(),
        };

        let out = provision_cluster(
            &deps,
            ProvisionClusterInput {
                cluster_id: Uuid::now_v7(),
                cluster_name: "acme-prod".into(),
                location: "nbg1".into(),
                server_type: "cpx21".into(),
                worker_count: 2,
                ssh_key: "ssh-ed25519 AAAA...".into(),
                user_data: "#cloud-config\n".into(),
                k3s_version: "v1.30.2+k3s1".into(),
            },
        )
        .await
        .expect("happy path should succeed");

        assert_eq!(out.workers.len(), 2);
        assert!(out.kubeconfig.contains("https://203.0.113"));

        let steps = progress.steps.lock().unwrap().clone();
        assert_eq!(
            steps,
            vec![
                "creating_control_plane",
                "waiting_cloud_init_cp",
                "installing_k3s_server",
                "creating_workers",
                "waiting_cloud_init_workers",
                "installing_k3s_agents",
                "collecting_kubeconfig",
                "done",
            ],
        );

        let created = hetzner.created.lock().unwrap();
        assert_eq!(created.len(), 3);
        assert_eq!(created[0].name, "acme-prod-cp-01");
        assert_eq!(created[1].name, "acme-prod-worker-00");
    }

    /// Sprint 2 ticket 07 — install_addon happy path with a fake
    /// HelmExecutor that records the parameters it received.
    struct FakeHelm {
        calls: Mutex<Vec<InstallParams>>,
        fail_with: Option<HelmError>,
    }
    #[async_trait]
    impl kubinate_integrations::helm::HelmExecutor for FakeHelm {
        async fn upgrade_install(
            &self,
            _kubeconfig: &std::path::Path,
            params: &InstallParams,
        ) -> Result<(), HelmError> {
            self.calls.lock().unwrap().push(params.clone());
            match &self.fail_with {
                Some(_) => Err(HelmError::Timeout(std::time::Duration::from_secs(1))),
                None => Ok(()),
            }
        }
    }

    #[tokio::test]
    async fn install_addon_happy_path_invokes_helm_once() {
        let helm = Arc::new(FakeHelm {
            calls: Mutex::new(Vec::new()),
            fail_with: None,
        });
        let progress = Arc::new(RecordingProgress {
            steps: Mutex::new(Vec::new()),
        });
        let deps = InstallAddonDeps {
            helm: helm.clone(),
            progress: progress.clone(),
        };
        let params = InstallParams {
            release: "ingress-nginx".into(),
            chart: "ingress-nginx".into(),
            repo: "https://kubernetes.github.io/ingress-nginx".into(),
            version: "4.10.0".into(),
            namespace: "ingress-nginx".into(),
            values_yaml: "controller:\n  replicaCount: 1\n".into(),
        };
        install_addon(
            &deps,
            InstallAddonInput {
                params: params.clone(),
                kubeconfig_path: std::path::PathBuf::from("/tmp/kubeconfig-fake"),
            },
        )
        .await
        .expect("happy path");

        let calls = helm.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].release, params.release);
        assert_eq!(
            progress.steps.lock().unwrap().clone(),
            vec!["installing", "done"]
        );
    }

    /// Recording fake `KubectlExecutor` for the scale-in tests.
    /// Tracks every drain/delete call and lets a test pre-script
    /// "already gone" node names so we can assert idempotency.
    struct FakeKubectl {
        drained: Mutex<Vec<String>>,
        deleted: Mutex<Vec<String>>,
        already_gone: std::collections::HashSet<String>,
    }

    impl FakeKubectl {
        fn new(already_gone: &[&str]) -> Self {
            Self {
                drained: Mutex::new(Vec::new()),
                deleted: Mutex::new(Vec::new()),
                already_gone: already_gone.iter().map(|s| (*s).to_string()).collect(),
            }
        }
    }

    #[async_trait]
    impl kubinate_integrations::kubectl::KubectlExecutor for FakeKubectl {
        async fn drain_node(
            &self,
            _kubeconfig_path: &std::path::Path,
            node_name: &str,
        ) -> Result<(), kubinate_integrations::kubectl::KubectlError> {
            self.drained.lock().unwrap().push(node_name.to_string());
            Ok(())
        }
        async fn delete_node(
            &self,
            _kubeconfig_path: &std::path::Path,
            node_name: &str,
        ) -> Result<(), kubinate_integrations::kubectl::KubectlError> {
            if self.already_gone.contains(node_name) {
                // Mirrors the real wrapper: NotFound → success.
                return Ok(());
            }
            self.deleted.lock().unwrap().push(node_name.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn scale_out_appends_workers_at_the_next_index() {
        // Existing cluster already has worker-00 and worker-01.
        // Scaling out by 2 should create worker-02 and worker-03.
        let hetzner = Arc::new(FakeHetzner {
            next: AtomicUsize::new(2000),
            created: Mutex::new(Vec::new()),
        });
        let ssh = Arc::new(FakeSsh);
        let progress = Arc::new(RecordingProgress {
            steps: Mutex::new(Vec::new()),
        });
        let deps = ProvisionDeps {
            hetzner: hetzner.clone(),
            ssh,
            progress: progress.clone(),
        };

        let out = scale_out(
            &deps,
            ScaleOutInput {
                cluster_id: Uuid::now_v7(),
                cluster_name: "acme-prod".into(),
                location: "nbg1".into(),
                server_type: "cpx21".into(),
                ssh_key: "ssh-ed25519 AAAA...".into(),
                user_data: "#cloud-config\n".into(),
                k3s_version: "v1.30.2+k3s1".into(),
                control_plane_endpoint: "203.0.113.10".into(),
                join_token: "K10test::server".into(),
                start_index: 2,
                count: 2,
            },
        )
        .await
        .expect("scale-out happy path");

        assert_eq!(out.workers.len(), 2);
        let created = hetzner.created.lock().unwrap();
        assert_eq!(created.len(), 2);
        assert_eq!(created[0].name, "acme-prod-worker-02");
        assert_eq!(created[1].name, "acme-prod-worker-03");

        assert_eq!(
            progress.steps.lock().unwrap().clone(),
            vec![
                "creating_workers",
                "waiting_cloud_init_workers",
                "installing_k3s_agents",
                "done",
            ],
        );
    }

    #[tokio::test]
    async fn scale_in_drains_then_deletes_each_target() {
        let hetzner = Arc::new(DestructibleHetzner::new(&[]));
        let kubectl = Arc::new(FakeKubectl::new(&[]));
        let progress = Arc::new(RecordingProgress {
            steps: Mutex::new(Vec::new()),
        });
        let deps = ScaleInDeps {
            hetzner: hetzner.clone(),
            kubectl: kubectl.clone(),
            progress: progress.clone(),
        };

        scale_in(
            &deps,
            ScaleInInput {
                cluster_id: Uuid::now_v7(),
                targets: vec![
                    WorkerToRemove {
                        server_id: ServerId(501),
                        node_name: "acme-prod-worker-04".into(),
                    },
                    WorkerToRemove {
                        server_id: ServerId(502),
                        node_name: "acme-prod-worker-05".into(),
                    },
                ],
                kubeconfig_path: std::path::PathBuf::from("/tmp/kubeconfig-fake"),
            },
        )
        .await
        .expect("scale-in happy path");

        assert_eq!(
            kubectl.drained.lock().unwrap().clone(),
            vec!["acme-prod-worker-04", "acme-prod-worker-05"],
        );
        assert_eq!(
            kubectl.deleted.lock().unwrap().clone(),
            vec!["acme-prod-worker-04", "acme-prod-worker-05"],
        );
        assert_eq!(
            hetzner.deleted.lock().unwrap().clone(),
            vec![ServerId(501), ServerId(502)],
        );
        assert_eq!(
            progress.steps.lock().unwrap().clone(),
            vec![
                "draining_nodes",
                "deleting_nodes",
                "destroying_servers",
                "done",
            ],
        );
    }

    #[tokio::test]
    async fn scale_in_is_idempotent_when_targets_already_gone() {
        // Re-run scenario: the k8s node and Hetzner server were
        // already removed by a prior partial run. Wrapper maps
        // `NotFound` to success on both sides; the workflow must
        // still complete successfully.
        let hetzner = Arc::new(DestructibleHetzner::new(&[701]));
        let kubectl = Arc::new(FakeKubectl::new(&["acme-prod-worker-06"]));
        let deps = ScaleInDeps {
            hetzner: hetzner.clone(),
            kubectl: kubectl.clone(),
            progress: Arc::new(NoopProgress),
        };
        scale_in(
            &deps,
            ScaleInInput {
                cluster_id: Uuid::now_v7(),
                targets: vec![WorkerToRemove {
                    server_id: ServerId(701),
                    node_name: "acme-prod-worker-06".into(),
                }],
                kubeconfig_path: std::path::PathBuf::from("/tmp/kubeconfig-fake"),
            },
        )
        .await
        .expect("re-run on partially-gone cluster should succeed");

        // drain still ran, but delete was skipped (already gone) and
        // the Hetzner server's 404 was swallowed by the activity.
        assert_eq!(kubectl.drained.lock().unwrap().len(), 1);
        assert!(kubectl.deleted.lock().unwrap().is_empty());
        assert!(hetzner.deleted.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn install_addon_surfaces_helm_errors() {
        let helm = Arc::new(FakeHelm {
            calls: Mutex::new(Vec::new()),
            fail_with: Some(HelmError::Timeout(std::time::Duration::from_secs(1))),
        });
        let deps = InstallAddonDeps {
            helm,
            progress: Arc::new(NoopProgress),
        };
        let params = InstallParams {
            release: "ingress-nginx".into(),
            chart: "ingress-nginx".into(),
            repo: "x".into(),
            version: "1".into(),
            namespace: "ingress-nginx".into(),
            values_yaml: String::new(),
        };
        let err = install_addon(
            &deps,
            InstallAddonInput {
                params,
                kubeconfig_path: std::path::PathBuf::from("/tmp/kc"),
            },
        )
        .await
        .expect_err("should propagate helm error");
        assert!(matches!(
            err,
            crate::activities::ActivityError::Helm(HelmError::Timeout(_))
        ));
    }
}
