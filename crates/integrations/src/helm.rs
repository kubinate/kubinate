//! Helm CLI wrapper used by the addon-install activity.
//!
//! Same shape as [`crate::ssh::OpensshExecutor`]: trait so unit tests
//! can swap in a fake, plus a real impl that shells out to the
//! `helm` binary. We don't use the Go client library because Sprint 2
//! values dependency footprint over fine-grained programmatic control.

use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::process::Command;

/// Errors surfaced by [`HelmExecutor`].
#[derive(Debug, Error)]
pub enum HelmError {
    /// `helm` exited non-zero.
    #[error("helm command failed (exit {code}): {stderr}")]
    NonZeroExit {
        /// Exit code.
        code: i32,
        /// Truncated stderr.
        stderr: String,
    },
    /// We could not invoke `helm` at all.
    #[error("spawn helm: {0}")]
    Transport(#[from] anyhow::Error),
    /// Per-call timeout fired.
    #[error("helm timed out after {0:?}")]
    Timeout(Duration),
}

/// Parameters for `helm upgrade --install`.
#[derive(Debug, Clone)]
pub struct InstallParams {
    /// Helm release name.
    pub release: String,
    /// Helm chart name (within the repo).
    pub chart: String,
    /// Helm chart repository URL.
    pub repo: String,
    /// Pinned chart version.
    pub version: String,
    /// Namespace (created if missing — `--create-namespace`).
    pub namespace: String,
    /// `--values` document, raw YAML. Empty string means defaults.
    pub values_yaml: String,
}

/// Trait the workflow uses. Unit tests provide a fake; production
/// uses [`HelmCliExecutor`].
#[async_trait]
pub trait HelmExecutor: Send + Sync {
    /// Run `helm upgrade --install`. Idempotent — re-runs converge to
    /// the requested state.
    async fn upgrade_install(
        &self,
        kubeconfig_path: &std::path::Path,
        params: &InstallParams,
    ) -> Result<(), HelmError>;

    /// Run `helm uninstall`. Idempotent — `release: not found` is
    /// treated as success so re-running after a prior uninstall is safe.
    async fn uninstall(
        &self,
        kubeconfig_path: &std::path::Path,
        release: &str,
        namespace: &str,
    ) -> Result<(), HelmError>;
}

/// Real `helm` binary wrapper.
pub struct HelmCliExecutor {
    /// Optional override for the `helm` binary location. Default is
    /// whatever `$PATH` resolves.
    pub helm_path: Option<PathBuf>,
    /// Per-call timeout. Helm installs of charts with many resources
    /// can take a minute or two; default to five.
    pub command_timeout: Duration,
}

impl HelmCliExecutor {
    /// Build with default settings.
    #[must_use]
    pub fn new(helm_path: Option<PathBuf>) -> Self {
        Self {
            helm_path,
            command_timeout: Duration::from_secs(300),
        }
    }
}

#[async_trait]
impl HelmExecutor for HelmCliExecutor {
    async fn upgrade_install(
        &self,
        kubeconfig_path: &std::path::Path,
        params: &InstallParams,
    ) -> Result<(), HelmError> {
        // Write the values YAML to a temp file. Helm reads it via
        // `--values <path>` so the chart lookup table never reaches
        // process argv.
        let values_path =
            std::env::temp_dir().join(format!("kubinate-helm-{}.yaml", uuid::Uuid::now_v7()));
        if !params.values_yaml.is_empty() {
            tokio::fs::write(&values_path, &params.values_yaml)
                .await
                .map_err(|e| HelmError::Transport(anyhow::anyhow!("write values: {e}")))?;
        }

        let bin = self
            .helm_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("helm"));
        let mut cmd = Command::new(&bin);
        cmd.env("KUBECONFIG", kubeconfig_path);
        cmd.arg("upgrade")
            .arg("--install")
            .arg(&params.release)
            .arg(&params.chart)
            .arg("--repo")
            .arg(&params.repo)
            .arg("--version")
            .arg(&params.version)
            .arg("--namespace")
            .arg(&params.namespace)
            .arg("--create-namespace")
            .arg("--wait")
            .arg("--timeout")
            .arg("5m");
        if !params.values_yaml.is_empty() {
            cmd.arg("--values").arg(&values_path);
        }
        cmd.kill_on_drop(true);

        let result = tokio::time::timeout(self.command_timeout, cmd.output()).await;

        // Best-effort cleanup of the values file regardless of outcome.
        let _ = tokio::fs::remove_file(&values_path).await;

        let output = match result {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(HelmError::Transport(anyhow::anyhow!("spawn helm: {e}"))),
            Err(_) => return Err(HelmError::Timeout(self.command_timeout)),
        };

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            Err(HelmError::NonZeroExit {
                code: output.status.code().unwrap_or(-1),
                stderr: stderr.chars().take(2048).collect(),
            })
        }
    }

    async fn uninstall(
        &self,
        kubeconfig_path: &std::path::Path,
        release: &str,
        namespace: &str,
    ) -> Result<(), HelmError> {
        let bin = self
            .helm_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("helm"));
        let mut cmd = Command::new(&bin);
        cmd.env("KUBECONFIG", kubeconfig_path);
        cmd.arg("uninstall")
            .arg(release)
            .arg("--namespace")
            .arg(namespace)
            .arg("--wait")
            .arg("--timeout")
            .arg("5m");
        cmd.kill_on_drop(true);

        let result = tokio::time::timeout(self.command_timeout, cmd.output()).await;

        let output = match result {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(HelmError::Transport(anyhow::anyhow!("spawn helm: {e}"))),
            Err(_) => return Err(HelmError::Timeout(self.command_timeout)),
        };

        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        // Treat "release: not found" as success — idempotent uninstall.
        if stderr.contains("release: not found") {
            return Ok(());
        }

        Err(HelmError::NonZeroExit {
            code: output.status.code().unwrap_or(-1),
            stderr: stderr.chars().take(2048).collect(),
        })
    }
}
