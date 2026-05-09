//! Agent-side connect-and-dispatch loop for the gRPC reverse tunnel.
//!
//! ## Lifecycle
//!
//! `run_connect_loop` is the top-level entry point. It never returns — the
//! agent is expected to run for the lifetime of the process. Each iteration:
//!
//! 1. Dials the control-plane URL (optionally with mTLS client cert).
//! 2. Opens the `AgentService.OpenStream` bidirectional RPC.
//! 3. Spawns a heartbeat ticker that fires immediately and every
//!    `AgentConfig::heartbeat_interval` thereafter.
//! 4. Reads `ServerToAgent` messages until the stream closes or errors.
//! 5. On clean close: reconnects immediately (attempt counter reset).
//!    On error: waits a full-jitter backoff capped at 60 s before retrying.

#![forbid(unsafe_code)]

use anyhow::Context as _;
use futures::StreamExt as _;
use kubinate_agent_proto::{
    AgentPayload, AgentServiceClient, AgentToServer, CommandKind, Heartbeat, ServerPayload,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};

/// Configuration for the agent reverse-tunnel connection.
pub struct AgentConfig {
    /// `https://` (prod with mTLS) or `http://` (dev) URL of the control plane.
    pub control_plane_url: String,
    /// UUID of the cluster this agent belongs to. Sent in every heartbeat.
    pub cluster_id: String,
    /// mTLS material. `None` → plain gRPC (dev/test only).
    pub mtls: Option<Arc<crate::cert::MtlsConfig>>,
    /// How often to send heartbeats. Default 30 s; short in tests.
    pub heartbeat_interval: Duration,
}

impl AgentConfig {
    /// Production constructor — 30 s heartbeat interval.
    pub fn new(
        control_plane_url: String,
        cluster_id: String,
        mtls: Option<crate::cert::MtlsConfig>,
    ) -> Self {
        Self {
            control_plane_url,
            cluster_id,
            mtls: mtls.map(Arc::new),
            heartbeat_interval: Duration::from_secs(30),
        }
    }
}

/// Outer reconnect loop. Never returns — call from `tokio::main`.
///
/// On clean session close (server ended the stream gracefully) the agent
/// reconnects immediately and resets the attempt counter. On any error the
/// agent applies full-jitter exponential backoff before the next attempt,
/// capped at 60 s.
pub async fn run_connect_loop(config: Arc<AgentConfig>) -> ! {
    let mut attempt = 0u32;
    loop {
        match build_and_run(&config).await {
            Ok(()) => {
                tracing::info!("session closed cleanly; reconnecting");
                attempt = 0;
            }
            Err(e) => {
                let delay = backoff_delay(attempt);
                tracing::warn!(
                    error = %e,
                    delay_ms = delay.as_millis(),
                    attempt,
                    "session error; backing off before reconnect"
                );
                tokio::time::sleep(delay).await;
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

async fn build_and_run(config: &AgentConfig) -> anyhow::Result<()> {
    let channel = build_channel(config).await?;
    run_session(config, channel).await
}

/// Build a tonic [`Channel`] to the control plane, applying mTLS when configured.
pub(crate) async fn build_channel(config: &AgentConfig) -> anyhow::Result<Channel> {
    let endpoint = Endpoint::try_from(config.control_plane_url.clone())
        .context("invalid control-plane URL")?;
    let endpoint = match &config.mtls {
        Some(m) => {
            let tls = ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(&m.ca_cert_pem))
                .identity(Identity::from_pem(&m.client_cert_pem, &m.client_key_pem));
            endpoint.tls_config(tls).context("apply TLS config")?
        }
        None => endpoint,
    };
    endpoint.connect().await.context("connect to control plane")
}

/// Run one session over an already-connected [`Channel`].
///
/// Returns `Ok(())` when the server closes the stream (clean reconnect trigger).
/// Returns `Err` on transport errors (triggers backoff).
pub(crate) async fn run_session(config: &AgentConfig, channel: Channel) -> anyhow::Result<()> {
    let mut client = AgentServiceClient::new(channel);
    let (outbound_tx, outbound_rx) = mpsc::channel::<AgentToServer>(32);

    let response_stream = client
        .open_stream(ReceiverStream::new(outbound_rx))
        .await
        .context("open_stream RPC failed")?;
    let mut inbound = response_stream.into_inner();

    // Heartbeat ticker — first tick fires immediately so the control plane
    // sees a liveness signal the moment the stream opens.
    let hb_tx = outbound_tx.clone();
    let cluster_id = config.cluster_id.clone();
    let hb_interval = config.heartbeat_interval;
    let heartbeat_task = tokio::spawn(async move {
        let mut ticker = tokio::time::interval(hb_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let msg = AgentToServer {
                payload: Some(AgentPayload::Heartbeat(Heartbeat {
                    cluster_id: cluster_id.clone(),
                    agent_version: env!("CARGO_PKG_VERSION").into(),
                    k3s_version: String::new(),
                    sent_at_ms: wall_clock_ms(),
                })),
            };
            if hb_tx.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(msg) = inbound.next().await {
        let msg = msg.context("inbound stream error")?;
        let Some(payload) = msg.payload else { continue };
        match payload {
            ServerPayload::HeartbeatAck(ack) => {
                let rtt_ms = wall_clock_ms().saturating_sub(ack.received_at_ms);
                tracing::debug!(rtt_ms, "heartbeat ack");
            }
            ServerPayload::Command(cmd) => {
                handle_command(cmd).await;
            }
        }
    }

    heartbeat_task.abort();
    Ok(())
}

async fn handle_command(cmd: kubinate_agent_proto::Command) {
    match cmd.kind {
        Some(CommandKind::GracefulRestart(r)) => {
            let grace = u64::try_from(r.grace_seconds.max(0)).unwrap_or(0);
            tracing::info!(grace_seconds = grace, "GracefulRestart received; draining");
            tokio::time::sleep(Duration::from_secs(grace)).await;
            // Systemd / k3s restarts the agent process after exit(0).
            std::process::exit(0);
        }
        None => {
            tracing::warn!("received Command with no kind variant; ignoring");
        }
    }
}

/// Full-jitter exponential backoff, capped at 60 s.
///
/// `delay = random(0, min(60_000ms, 500ms × 2^attempt))`
pub(crate) fn backoff_delay(attempt: u32) -> Duration {
    let base_ms: u64 = 500;
    let cap_ms: u64 = 60_000;
    // Cap the exponent at 7 so the arithmetic stays well within u64 range
    // before the min(cap_ms) is applied (500 × 2^7 = 64_000 ≥ cap).
    let ceiling = base_ms.saturating_mul(1u64 << attempt.min(7)).min(cap_ms);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let jitter_ms = (rand::random::<f64>() * ceiling as f64) as u64;
    Duration::from_millis(jitter_ms)
}

fn wall_clock_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(dur.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::pin::Pin;

    use async_trait::async_trait;
    use futures::Stream;
    use kubinate_agent_proto::{
        AgentService, AgentServiceServer, HeartbeatAck, ServerPayload, ServerToAgent,
    };
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;
    use tonic::{
        transport::{Endpoint, Server, Uri},
        Request, Response, Status, Streaming,
    };

    // ── echo service ─────────────────────────────────────────────────────────

    /// Acks the first heartbeat received then drops its sender, which closes
    /// the server-side stream so `run_session` exits with `Ok(())`.
    struct OneHeartbeatService;

    #[async_trait]
    impl AgentService for OneHeartbeatService {
        type OpenStreamStream =
            Pin<Box<dyn Stream<Item = Result<ServerToAgent, Status>> + Send + 'static>>;

        async fn open_stream(
            &self,
            request: Request<Streaming<AgentToServer>>,
        ) -> Result<Response<Self::OpenStreamStream>, Status> {
            let mut inbound = request.into_inner();
            let (tx, rx) = mpsc::channel::<Result<ServerToAgent, Status>>(1);
            tokio::spawn(async move {
                if let Some(Ok(msg)) = inbound.next().await {
                    if let Some(AgentPayload::Heartbeat(hb)) = msg.payload {
                        let _ = tx
                            .send(Ok(ServerToAgent {
                                payload: Some(ServerPayload::HeartbeatAck(HeartbeatAck {
                                    received_at_ms: hb.sent_at_ms + 1,
                                })),
                            }))
                            .await;
                    }
                }
                // tx drops here → server stream closes → run_session returns Ok(())
            });
            Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
        }
    }

    // ── helpers ───────────────────────────────────────────────────────────────

    async fn loopback_channel<S>(service: S) -> Channel
    where
        S: AgentService,
    {
        let (client_io, server_io) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            Server::builder()
                .add_service(AgentServiceServer::new(service))
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server_io)))
                .await
                .expect("server task crashed");
        });
        let mut io_cell = Some(client_io);
        Endpoint::try_from("http://kubinate-agent-test")
            .unwrap()
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let io = io_cell.take().expect("connector called once");
                async move { Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(io)) }
            }))
            .await
            .expect("loopback channel failed")
    }

    fn test_config() -> AgentConfig {
        AgentConfig {
            control_plane_url: "http://unused-test".into(),
            cluster_id: "test-cluster-0001".into(),
            mtls: None,
            // Fire first heartbeat immediately; no need to wait 30 s.
            heartbeat_interval: Duration::from_millis(10),
        }
    }

    // ── tests ─────────────────────────────────────────────────────────────────

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_round_trips_heartbeat_and_exits_on_server_close() {
        let channel = loopback_channel(OneHeartbeatService).await;
        let config = test_config();

        let result = tokio::time::timeout(Duration::from_secs(5), run_session(&config, channel))
            .await
            .expect("run_session did not complete within 5 s");

        assert!(result.is_ok(), "expected clean close, got: {result:?}");
    }

    #[test]
    fn backoff_delay_never_exceeds_cap() {
        for attempt in 0..30 {
            let d = backoff_delay(attempt);
            assert!(
                d <= Duration::from_secs(60),
                "attempt {attempt}: delay {d:?} exceeds 60 s cap"
            );
        }
    }

    #[test]
    fn backoff_delay_is_zero_or_positive() {
        for attempt in 0..10 {
            let d = backoff_delay(attempt);
            assert!(
                d >= Duration::ZERO,
                "attempt {attempt}: negative delay {d:?}"
            );
        }
    }
}
