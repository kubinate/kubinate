//! SSH execution trait used by the k3s install activities.
//!
//! The real implementation (native SSH via the `russh` crate or a
//! wrapped `ssh` binary) lands behind the feature-flagged integration
//! test from ticket 03. Until then the workflow takes an
//! implementation of [`SshExecutor`] through dependency injection so
//! unit tests can run against a [`crate::ssh::tests::FakeSsh`].

use std::{path::PathBuf, time::Duration};

use async_trait::async_trait;
use thiserror::Error;
use tokio::process::Command;

/// Errors surfaced by an [`SshExecutor`] implementation.
#[derive(Debug, Error)]
pub enum SshError {
    /// The remote command returned a non-zero exit code.
    #[error("remote command failed with exit code {code}: {stderr}")]
    NonZeroExit {
        /// Remote exit code.
        code: i32,
        /// Truncated stderr for diagnostics.
        stderr: String,
    },
    /// The SSH connection could not be established.
    #[error("ssh connect: {0}")]
    Connect(String),
    /// Catch-all for transport / protocol errors.
    #[error(transparent)]
    Transport(#[from] anyhow::Error),
}

/// Target host for an SSH command.
#[derive(Debug, Clone)]
pub struct SshTarget {
    /// Hostname / IP the client will dial.
    pub host: String,
    /// Remote user (defaults to `kubinate`).
    pub user: String,
}

/// Narrow trait the provisioning activities use. Deliberately small so
/// test fakes can implement it without pretending to be a full SSH
/// library.
#[async_trait]
pub trait SshExecutor: Send + Sync {
    /// Run `command` on `target` and return its stdout.
    ///
    /// Implementations must return [`SshError::NonZeroExit`] (not
    /// `Transport`) when the remote exit code is non-zero so retry
    /// policies can distinguish "command failed deterministically"
    /// from "transport flake".
    async fn run(&self, target: &SshTarget, command: &str) -> Result<String, SshError>;
}

/// Real SSH executor that shells out to the system `ssh` binary.
///
/// Why subprocess and not `russh`: the `ssh` binary is universally
/// available on every CI image and the production VPS, supports
/// SSH-agent forwarding without us having to implement RFC 4252, and
/// is auditable in a single command line. Latency is fine for our
/// activity granularity (a few seconds per call). When we move to
/// long-lived agent connections, swapping in `russh` is local to
/// this struct.
pub struct OpensshExecutor {
    /// Path to a private key the local `ssh` will offer.
    pub identity_file: Option<PathBuf>,
    /// Per-call timeout. The provisioning workflow runs slow commands
    /// (k3s install) so the default has to be generous.
    pub command_timeout: Duration,
    /// How many times to retry a `Connection refused` / `kex_exchange`
    /// failure before giving up. Cloud-init can leave sshd briefly
    /// unreachable after first boot; we want to ride that out without
    /// surfacing it to the workflow as a failure.
    pub connect_retries: u32,
    /// Wait between retries.
    pub connect_backoff: Duration,
}

impl OpensshExecutor {
    /// Build a default-configured executor.
    #[must_use]
    pub fn new(identity_file: Option<PathBuf>) -> Self {
        Self {
            identity_file,
            command_timeout: Duration::from_secs(300),
            connect_retries: 6,
            connect_backoff: Duration::from_secs(10),
        }
    }

    fn build_command(&self, target: &SshTarget, command: &str) -> Command {
        let mut cmd = Command::new("ssh");
        // Non-interactive options: never prompt, never read stdin
        // beyond what we hand it.
        cmd.arg("-o").arg("BatchMode=yes");
        cmd.arg("-o").arg("StrictHostKeyChecking=accept-new");
        cmd.arg("-o")
            .arg("UserKnownHostsFile=/tmp/kubinate-known-hosts");
        cmd.arg("-o").arg(format!("ConnectTimeout={}", 30));
        cmd.arg("-o").arg("ServerAliveInterval=30");

        if let Some(ref key) = self.identity_file {
            cmd.arg("-i").arg(key);
        }

        cmd.arg(format!("{}@{}", target.user, target.host));
        cmd.arg("--").arg(command);
        cmd.kill_on_drop(true);
        cmd
    }
}

#[async_trait]
impl SshExecutor for OpensshExecutor {
    async fn run(&self, target: &SshTarget, command: &str) -> Result<String, SshError> {
        let mut last_connect_err: Option<SshError> = None;

        for attempt in 0..=self.connect_retries {
            let result = tokio::time::timeout(
                self.command_timeout,
                self.build_command(target, command).output(),
            )
            .await;

            let output = match result {
                Ok(Ok(out)) => out,
                Ok(Err(e)) => {
                    return Err(SshError::Transport(anyhow::anyhow!("spawn ssh: {e}")));
                }
                Err(_) => {
                    return Err(SshError::Transport(anyhow::anyhow!(
                        "ssh command timed out after {:?}",
                        self.command_timeout
                    )));
                }
            };

            let exit_code = output.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }

            // Distinguish "remote command failed" (NonZeroExit, not
            // retryable) from "transport / connect flake" (retryable
            // up to `connect_retries`). `ssh` returns 255 for its own
            // errors and the remote command's exit code otherwise.
            let is_transport = exit_code == 255
                || stderr.contains("Connection refused")
                || stderr.contains("kex_exchange_identification")
                || stderr.contains("Connection timed out")
                || stderr.contains("Connection closed by");

            if is_transport && attempt < self.connect_retries {
                tracing::warn!(
                    target = %target.host,
                    attempt = attempt + 1,
                    "ssh transport flake, retrying",
                );
                last_connect_err = Some(SshError::Connect(stderr.clone()));
                tokio::time::sleep(self.connect_backoff).await;
                continue;
            }
            if is_transport {
                return Err(SshError::Connect(stderr));
            }
            return Err(SshError::NonZeroExit {
                code: exit_code,
                stderr: stderr.chars().take(2048).collect(),
            });
        }

        Err(last_connect_err
            .unwrap_or_else(|| SshError::Transport(anyhow::anyhow!("unreachable retry exit"))))
    }
}
