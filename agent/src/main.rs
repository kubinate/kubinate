//! Kubinate cluster-resident agent.
//!
//! Dials the control plane via mTLS gRPC, opens the `AgentService.OpenStream`
//! bidirectional RPC, and runs a heartbeat + command-dispatch loop that
//! reconnects on error with full-jitter exponential backoff.
//!
//! Required env vars:
//!   `KUBINATE__CONTROL_PLANE_URL` — `https://` URL of the control-plane gRPC port.
//!   `KUBINATE__CLUSTER_ID`        — UUID of the cluster this agent belongs to.
//!
//! mTLS env vars (all three required for a production deploy; absent → plain gRPC,
//! which is only safe for local dev / integration tests):
//!   `KUBINATE__CERT_PATH` — PEM client certificate chain.
//!   `KUBINATE__KEY_PATH`  — PEM private key matching the leaf cert.
//!   `KUBINATE__CA_PATH`   — PEM CA cert used to verify the control-plane server.

#![forbid(unsafe_code)]

mod cert;
mod connect;

use std::{path::PathBuf, sync::Arc};

use kubinate_platform::telemetry;

fn required_env(var: &str) -> anyhow::Result<String> {
    std::env::var(var).map_err(|_| anyhow::anyhow!("required env var {var} is not set"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init("kubinate-agent")?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "kubinate-agent starting"
    );

    let control_plane_url = required_env("KUBINATE__CONTROL_PLANE_URL")?;
    let cluster_id = required_env("KUBINATE__CLUSTER_ID")?;

    let mtls = match (
        std::env::var("KUBINATE__CERT_PATH").ok(),
        std::env::var("KUBINATE__KEY_PATH").ok(),
        std::env::var("KUBINATE__CA_PATH").ok(),
    ) {
        (Some(cert), Some(key), Some(ca)) => {
            tracing::info!("mTLS enabled");
            Some(cert::MtlsConfig::load(
                &PathBuf::from(cert),
                &PathBuf::from(key),
                &PathBuf::from(ca),
            )?)
        }
        _ => {
            tracing::warn!(
                "KUBINATE__CERT_PATH / KEY_PATH / CA_PATH not all set; \
                 running without mTLS (dev/test only)"
            );
            None
        }
    };

    let config = Arc::new(connect::AgentConfig::new(
        control_plane_url,
        cluster_id,
        mtls,
    ));
    connect::run_connect_loop(config).await
}
