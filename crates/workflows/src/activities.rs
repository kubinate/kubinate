//! Activities — the side-effecting steps of the provisioning workflow.
//!
//! Every function here is invoked by the workflow (today via the
//! in-process runner in [`crate::runner`], eventually via Temporal
//! once the SDK spike from ADR-0003 concludes) with dependency-
//! injected [`HetznerProvider`] / [`SshExecutor`] implementations.
//! This keeps the activities unit-testable without needing a live
//! Hetzner project or a running SSH server.
//!
//! Naming mirrors ADR-0003: each function name corresponds to one of
//! the activities listed there.

use std::time::Duration;

use kubinate_integrations::{
    helm::{HelmError, HelmExecutor, InstallParams},
    hetzner::{
        CreateServerParams, HetznerError, HetznerProvider, HetznerServer, ServerId, ServerStatus,
    },
    kubectl::{KubectlError, KubectlExecutor},
    ssh::{SshError, SshExecutor, SshTarget},
};
use thiserror::Error;
use uuid::Uuid;

/// Errors surfaced by activities. The variants deliberately preserve
/// the distinction between a transient Hetzner error (which retry
/// policies backoff against) and a deterministic failure (which
/// compensates immediately).
#[derive(Debug, Error)]
pub enum ActivityError {
    /// Hetzner API call failed.
    #[error(transparent)]
    Hetzner(#[from] HetznerError),
    /// Remote SSH command failed.
    #[error(transparent)]
    Ssh(#[from] SshError),
    /// Helm install / upgrade failed.
    #[error(transparent)]
    Helm(#[from] HelmError),
    /// kubectl drain / delete failed.
    #[error(transparent)]
    Kubectl(#[from] KubectlError),
    /// Cloud-init never reported ready before the deadline.
    #[error("cloud-init timed out after {attempts} polls")]
    CloudInitTimeout {
        /// Number of polls attempted before giving up.
        attempts: usize,
    },
    /// Generic transport / unexpected error surfaced as a non-retryable.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl ActivityError {
    /// Whether the caller should retry this error. Transient Hetzner
    /// network failures and SSH transport flakes are retryable;
    /// non-zero exits and timeouts are not.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            ActivityError::Hetzner(HetznerError::Transport(_)) => true,
            ActivityError::Hetzner(HetznerError::Status { status, .. }) => {
                matches!(*status, 408 | 425 | 429 | 500 | 502 | 503 | 504)
            }
            ActivityError::Ssh(SshError::Connect(_) | SshError::Transport(_)) => true,
            _ => false,
        }
    }
}

/// Upper bound on cloud-init poll attempts. Each attempt waits 5s
/// between polls (see [`wait_for_cloud_init`]), so 60 * 5s = 5 min.
pub const CLOUD_INIT_MAX_ATTEMPTS: usize = 60;

/// Delay between cloud-init polls.
pub const CLOUD_INIT_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Compensation activity used by [`crate::workflows::destroy_cluster`].
///
/// Treats Hetzner's `404 not found` as success — the workflow needs
/// to be re-runnable on a partially-destroyed cluster (ticket 05 AC).
/// Other errors (network, 5xx, auth) propagate so retry / alerting
/// can fire normally.
pub async fn hetzner_delete_server_idempotent(
    provider: &dyn HetznerProvider,
    id: kubinate_integrations::hetzner::ServerId,
) -> Result<(), ActivityError> {
    match provider.delete_server(id).await {
        Ok(()) => Ok(()),
        Err(HetznerError::Status { status: 404, .. }) => {
            tracing::info!(server_id = ?id, "server already gone, treating as success");
            Ok(())
        }
        Err(other) => Err(ActivityError::Hetzner(other)),
    }
}

/// ADR-0003 activity #1. Create a Hetzner Cloud server.
pub async fn hetzner_create_server(
    provider: &dyn HetznerProvider,
    params: CreateServerParams,
) -> Result<ServerId, ActivityError> {
    tracing::info!(
        name = %params.name,
        region = %params.location,
        server_type = %params.server_type,
        idempotency_key = %params.idempotency_key,
        "creating hetzner server",
    );
    Ok(provider.create_server(params).await?)
}

/// ADR-0003 activity #2. Poll a server until cloud-init completes.
///
/// Uses a fixed-interval poll loop rather than backoff: cloud-init
/// takes a fairly predictable amount of time and we want the common
/// case to finish quickly once it's done.
pub async fn wait_for_cloud_init(
    provider: &dyn HetznerProvider,
    server_id: ServerId,
) -> Result<HetznerServer, ActivityError> {
    for attempt in 0..CLOUD_INIT_MAX_ATTEMPTS {
        let server = provider.get_server(server_id).await?;
        match server.status {
            ServerStatus::Ready => return Ok(server),
            ServerStatus::Failed => {
                return Err(ActivityError::Other(anyhow::anyhow!(
                    "server {server_id:?} reported Failed status during boot",
                )))
            }
            ServerStatus::Initializing => {
                tracing::debug!(
                    server_id = ?server_id,
                    attempt = attempt + 1,
                    "cloud-init still running",
                );
                tokio::time::sleep(CLOUD_INIT_POLL_INTERVAL).await;
            }
        }
    }
    Err(ActivityError::CloudInitTimeout {
        attempts: CLOUD_INIT_MAX_ATTEMPTS,
    })
}

/// ADR-0003 activity #3. Install k3s in server mode on a freshly
/// booted VM and extract the join token.
///
/// The concrete command wraps `k3sup` so we never paste a literal
/// shell pipeline (per ADR-0003). The returned token is already
/// narrowed to the `K10...` prefix; the caller is responsible for
/// treating it as a [`secrecy::SecretString`] beyond this point.
pub async fn ssh_install_k3s_server(
    ssh: &dyn SshExecutor,
    target: SshTarget,
    k3s_version: &str,
) -> Result<String, ActivityError> {
    let cmd = format!(
        "k3sup install --local --k3s-version {version}",
        version = shell_escape(k3s_version),
    );
    let _installed = ssh.run(&target, &cmd).await?;

    // `cat` of the node-token file is the only place the bootstrap
    // token is stable. k3sup doesn't expose it directly on stdout.
    let token = ssh
        .run(&target, "sudo cat /var/lib/rancher/k3s/server/node-token")
        .await?;
    Ok(token.trim().to_string())
}

/// ADR-0003 activity #4. Join a freshly booted server as a k3s agent
/// pointing at the existing control plane.
pub async fn ssh_install_k3s_agent(
    ssh: &dyn SshExecutor,
    worker: SshTarget,
    control_plane_ip: &str,
    join_token: &str,
    k3s_version: &str,
) -> Result<(), ActivityError> {
    let cmd = format!(
        "k3sup join --server-ip {cp} --k3s-version {ver} --token {tok}",
        cp = shell_escape(control_plane_ip),
        ver = shell_escape(k3s_version),
        tok = shell_escape(join_token),
    );
    let _out = ssh.run(&worker, &cmd).await?;
    Ok(())
}

/// ADR-0003 activity #5. Collect the kubeconfig from the control
/// plane once everything is joined. The returned string is caller's
/// responsibility to encrypt + store (ticket 04).
pub async fn collect_kubeconfig(
    ssh: &dyn SshExecutor,
    control_plane: SshTarget,
    public_endpoint: &str,
) -> Result<String, ActivityError> {
    // k3s writes the kubeconfig with the internal 127.0.0.1 endpoint;
    // we rewrite it to the reachable public endpoint on the way out.
    let raw = ssh
        .run(&control_plane, "sudo cat /etc/rancher/k3s/k3s.yaml")
        .await?;
    Ok(raw.replace("https://127.0.0.1:6443", public_endpoint))
}

/// Sprint 3 ticket 08. Drain a worker node ahead of deletion.
/// Idempotent — the kubectl wrapper treats `NotFound` as success so a
/// re-run after a partial scale-in completes cleanly.
pub async fn kubectl_drain_node(
    kubectl: &dyn KubectlExecutor,
    kubeconfig_path: &std::path::Path,
    node_name: &str,
) -> Result<(), ActivityError> {
    tracing::info!(node = %node_name, "draining node");
    kubectl.drain_node(kubeconfig_path, node_name).await?;
    Ok(())
}

/// Sprint 3 ticket 08. Delete a worker node from the API server. Runs
/// after `kubectl_drain_node` and before the Hetzner-side delete so
/// kube-state stays consistent with infrastructure.
pub async fn kubectl_delete_node(
    kubectl: &dyn KubectlExecutor,
    kubeconfig_path: &std::path::Path,
    node_name: &str,
) -> Result<(), ActivityError> {
    tracing::info!(node = %node_name, "deleting node");
    kubectl.delete_node(kubeconfig_path, node_name).await?;
    Ok(())
}

/// Sprint 2 ticket 07. Run `helm upgrade --install` against the
/// cluster's kubeconfig with the catalog-derived `InstallParams`.
pub async fn helm_install_chart(
    helm: &dyn HelmExecutor,
    kubeconfig_path: &std::path::Path,
    params: &InstallParams,
) -> Result<(), ActivityError> {
    tracing::info!(
        release = %params.release,
        chart = %params.chart,
        version = %params.version,
        namespace = %params.namespace,
        "helm install starting",
    );
    helm.upgrade_install(kubeconfig_path, params).await?;
    Ok(())
}

fn shell_escape(value: &str) -> String {
    // Simple single-quote escape; sufficient for the narrow set of
    // inputs (k3s version, token, IP) that reach the SSH command line.
    // A proper shellwords crate is warranted once inputs broaden.
    let escaped = value.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// Build a [`CreateServerParams`] for the control plane of a cluster.
///
/// Kept here (not inline in the workflow) so tests can assert on the
/// exact parameters the workflow would pass to Hetzner without having
/// to re-derive the name/label scheme.
#[must_use]
pub fn control_plane_params(
    cluster_id: Uuid,
    cluster_name: &str,
    server_type: &str,
    location: &str,
    ssh_key: &str,
    user_data: &str,
) -> CreateServerParams {
    CreateServerParams {
        name: format!("{cluster_name}-cp-01"),
        server_type: server_type.to_string(),
        location: location.to_string(),
        ssh_key: ssh_key.to_string(),
        user_data: user_data.to_string(),
        idempotency_key: cluster_id,
    }
}

/// Build [`CreateServerParams`] for one of N workers.
#[must_use]
pub fn worker_params(
    cluster_id: Uuid,
    cluster_name: &str,
    index: usize,
    server_type: &str,
    location: &str,
    ssh_key: &str,
    user_data: &str,
) -> CreateServerParams {
    CreateServerParams {
        name: format!("{cluster_name}-worker-{index:02}"),
        server_type: server_type.to_string(),
        location: location.to_string(),
        ssh_key: ssh_key.to_string(),
        user_data: user_data.to_string(),
        // Worker idempotency is per-cluster + index so a retried
        // worker create doesn't collide with the control plane key.
        idempotency_key: Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("kubinate.worker:{cluster_id}:{index}").as_bytes(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use kubinate_integrations::hetzner::HetznerServer;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    };

    struct FakeHetzner {
        created: Mutex<Vec<CreateServerParams>>,
        next_id: AtomicUsize,
        // ready_after[i] = how many `get_server` calls before this id
        // reports Ready.
        ready_after: Mutex<std::collections::HashMap<u64, usize>>,
        get_calls: Mutex<std::collections::HashMap<u64, usize>>,
    }

    impl FakeHetzner {
        fn new() -> Self {
            Self {
                created: Mutex::new(Vec::new()),
                next_id: AtomicUsize::new(1000),
                ready_after: Mutex::new(std::collections::HashMap::new()),
                get_calls: Mutex::new(std::collections::HashMap::new()),
            }
        }

        fn schedule_ready(&self, id: ServerId, after_calls: usize) {
            self.ready_after.lock().unwrap().insert(id.0, after_calls);
        }
    }

    #[async_trait]
    impl HetznerProvider for FakeHetzner {
        async fn create_server(
            &self,
            params: CreateServerParams,
        ) -> Result<ServerId, HetznerError> {
            let id = ServerId(self.next_id.fetch_add(1, Ordering::SeqCst) as u64);
            self.created.lock().unwrap().push(params);
            self.schedule_ready(id, 1); // default: ready on the 2nd poll
            Ok(id)
        }

        async fn get_server(&self, id: ServerId) -> Result<HetznerServer, HetznerError> {
            let mut calls = self.get_calls.lock().unwrap();
            let n = calls.entry(id.0).and_modify(|c| *c += 1).or_insert(1);
            let threshold = self
                .ready_after
                .lock()
                .unwrap()
                .get(&id.0)
                .copied()
                .unwrap_or(0);
            let status = if *n > threshold {
                ServerStatus::Ready
            } else {
                ServerStatus::Initializing
            };
            Ok(HetznerServer {
                id,
                status,
                ipv4: Some(format!("203.0.113.{}", id.0 % 250)),
                private_ipv4: Some(format!("10.20.1.{}", id.0 % 250)),
            })
        }

        async fn delete_server(&self, _id: ServerId) -> Result<(), HetznerError> {
            Ok(())
        }
    }

    struct FakeSsh {
        script: Mutex<std::collections::HashMap<String, String>>,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl FakeSsh {
        fn new() -> Self {
            Self {
                script: Mutex::new(std::collections::HashMap::new()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn scripted(pairs: &[(&str, &str)]) -> Self {
            let me = Self::new();
            for (cmd, out) in pairs {
                me.script
                    .lock()
                    .unwrap()
                    .insert((*cmd).into(), (*out).into());
            }
            me
        }
    }

    #[async_trait]
    impl SshExecutor for FakeSsh {
        async fn run(&self, target: &SshTarget, command: &str) -> Result<String, SshError> {
            self.calls
                .lock()
                .unwrap()
                .push((target.host.clone(), command.to_string()));
            // Match by exact command; fall back to matching the first
            // whitespace-delimited token so test setups don't have to
            // spell out every full command line.
            let script = self.script.lock().unwrap();
            if let Some(v) = script.get(command) {
                return Ok(v.clone());
            }
            let head = command.split_whitespace().next().unwrap_or("");
            if let Some(v) = script.get(head) {
                return Ok(v.clone());
            }
            Ok(String::new())
        }
    }

    #[tokio::test]
    async fn create_server_passes_params_through() {
        let h = FakeHetzner::new();
        let params = control_plane_params(
            Uuid::now_v7(),
            "acme-prod",
            "cpx21",
            "nbg1",
            "ssh-ed25519 AAAA...",
            "#cloud-config\n",
        );
        let id = hetzner_create_server(&h, params.clone()).await.unwrap();
        assert!(id.0 >= 1000);
        let created = h.created.lock().unwrap();
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].name, "acme-prod-cp-01");
    }

    #[tokio::test]
    async fn wait_for_cloud_init_polls_until_ready() {
        let h = Arc::new(FakeHetzner::new());
        let params = control_plane_params(
            Uuid::now_v7(),
            "acme",
            "cpx21",
            "nbg1",
            "ssh-ed25519 k",
            "#cloud-config\n",
        );
        let id = h.create_server(params).await.unwrap();
        // Default schedule: ready after 1 poll => second poll succeeds.
        let server = wait_for_cloud_init(&*h, id).await.unwrap();
        assert_eq!(server.status, ServerStatus::Ready);
    }

    #[tokio::test]
    async fn ssh_install_k3s_server_returns_token() {
        let ssh = FakeSsh::scripted(&[("k3sup", ""), ("sudo", "K10abcdef::server\n")]);
        let token = ssh_install_k3s_server(
            &ssh,
            SshTarget {
                host: "203.0.113.10".into(),
                user: "kubinate".into(),
            },
            "v1.30.2+k3s1",
        )
        .await
        .unwrap();
        assert_eq!(token, "K10abcdef::server");
    }

    #[tokio::test]
    async fn collect_kubeconfig_rewrites_server_url() {
        let ssh =
            FakeSsh::scripted(&[("sudo", "apiVersion: v1\nserver: https://127.0.0.1:6443\n")]);
        let kc = collect_kubeconfig(
            &ssh,
            SshTarget {
                host: "203.0.113.10".into(),
                user: "kubinate".into(),
            },
            "https://cp.example.com:6443",
        )
        .await
        .unwrap();
        assert!(kc.contains("https://cp.example.com:6443"));
        assert!(!kc.contains("127.0.0.1"));
    }

    #[test]
    fn retryable_hetzner_429_is_flagged_for_retry() {
        let err = ActivityError::Hetzner(HetznerError::Status {
            status: 429,
            body: "rate limited".into(),
        });
        assert!(err.is_retryable());
    }

    #[test]
    fn deterministic_errors_are_not_retried() {
        let err = ActivityError::CloudInitTimeout { attempts: 60 };
        assert!(!err.is_retryable());
    }
}
