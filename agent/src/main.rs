//! Kubinate cluster-resident agent.
//!
//! Phase-0 shape: a stub binary that logs and exits. The full agent
//! (mTLS gRPC reverse tunnel to the control plane, health reporting,
//! command execution) ships in Phase 2 after the reverse-tunnel design
//! spike (brief §12).

#![forbid(unsafe_code)]

use kubinate_platform::telemetry;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init("kubinate-agent")?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "agent starting (stub)");
    tracing::warn!("agent is a Phase-2 deliverable; this is a skeleton binary");
    Ok(())
}
