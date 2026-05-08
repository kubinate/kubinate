//! Kubectl wrapper used by the scale-in workflow's drain step
//! (Sprint 3 ticket 08).
//!
//! Same shape as [`crate::helm::HelmCliExecutor`] and
//! [`crate::ssh::OpensshExecutor`]: trait + a real impl that shells
//! out to the system `kubectl` binary. Tests inject a fake.

use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::process::Command;

/// Errors surfaced by [`KubectlExecutor`].
#[derive(Debug, Error)]
pub enum KubectlError {
    /// `kubectl` exited non-zero.
    #[error("kubectl failed (exit {code}): {stderr}")]
    NonZeroExit {
        /// Exit code.
        code: i32,
        /// Truncated stderr.
        stderr: String,
    },
    /// We could not spawn `kubectl`.
    #[error("spawn kubectl: {0}")]
    Transport(#[from] anyhow::Error),
    /// Per-call timeout fired.
    #[error("kubectl timed out after {0:?}")]
    Timeout(Duration),
}

/// Trait the workflow uses. Tests provide a fake; production uses
/// [`KubectlCliExecutor`].
#[async_trait]
pub trait KubectlExecutor: Send + Sync {
    /// `kubectl drain <node> --ignore-daemonsets --delete-emptydir-data`.
    /// Idempotent: a second call against an already-drained node
    /// returns success.
    async fn drain_node(
        &self,
        kubeconfig_path: &std::path::Path,
        node_name: &str,
    ) -> Result<(), KubectlError>;

    /// `kubectl delete node <node>`. Treated as idempotent — `not
    /// found` exit codes are mapped to success.
    async fn delete_node(
        &self,
        kubeconfig_path: &std::path::Path,
        node_name: &str,
    ) -> Result<(), KubectlError>;
}

/// Real `kubectl` binary wrapper.
pub struct KubectlCliExecutor {
    /// Optional override for the binary path.
    pub kubectl_path: Option<PathBuf>,
    /// Per-call timeout. Drain on a busy node can take a couple of
    /// minutes while pods reschedule; default to four.
    pub command_timeout: Duration,
}

impl KubectlCliExecutor {
    /// Build with default settings.
    #[must_use]
    pub fn new(kubectl_path: Option<PathBuf>) -> Self {
        Self {
            kubectl_path,
            command_timeout: Duration::from_secs(240),
        }
    }

    fn build_command(&self, kubeconfig: &std::path::Path) -> Command {
        let bin = self
            .kubectl_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("kubectl"));
        let mut cmd = Command::new(bin);
        cmd.env("KUBECONFIG", kubeconfig);
        cmd.kill_on_drop(true);
        cmd
    }

    async fn run(
        &self,
        mut cmd: Command,
        ignore_not_found: bool,
    ) -> Result<(), KubectlError> {
        let result = tokio::time::timeout(self.command_timeout, cmd.output()).await;
        let output = match result {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => {
                return Err(KubectlError::Transport(anyhow::anyhow!(
                    "spawn kubectl: {e}"
                )))
            }
            Err(_) => return Err(KubectlError::Timeout(self.command_timeout)),
        };

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // `kubectl delete node` returns exit 1 + "NotFound" when the
        // node is already gone — that's our success path for the
        // re-run case.
        if ignore_not_found
            && (stderr.contains("NotFound") || stderr.contains("not found"))
        {
            return Ok(());
        }

        Err(KubectlError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: stderr.chars().take(2048).collect(),
        })
    }
}

#[async_trait]
impl KubectlExecutor for KubectlCliExecutor {
    async fn drain_node(
        &self,
        kubeconfig_path: &std::path::Path,
        node_name: &str,
    ) -> Result<(), KubectlError> {
        let mut cmd = self.build_command(kubeconfig_path);
        cmd.arg("drain")
            .arg(node_name)
            .arg("--ignore-daemonsets")
            .arg("--delete-emptydir-data")
            .arg("--force")
            .arg("--timeout=180s");
        self.run(cmd, /* ignore_not_found = */ true).await
    }

    async fn delete_node(
        &self,
        kubeconfig_path: &std::path::Path,
        node_name: &str,
    ) -> Result<(), KubectlError> {
        let mut cmd = self.build_command(kubeconfig_path);
        cmd.arg("delete").arg("node").arg(node_name);
        self.run(cmd, /* ignore_not_found = */ true).await
    }
}
