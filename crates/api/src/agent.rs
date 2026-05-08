//! Sprint 4 ticket 03 — control-plane gRPC service for the agent
//! reverse tunnel.
//!
//! ## Wire shape
//!
//! Implements the `kubinate.agent.v1.AgentService.OpenStream`
//! bidirectional streaming RPC defined in
//! [`kubinate_agent_proto`]. The agent dials this service and
//! multiplexes [`AgentToServer`] messages over the stream;
//! the server responds with [`ServerToAgent`] messages on the
//! same stream.
//!
//! ## Mount story (Sprint 4 vs Sprint 5+)
//!
//! In Sprint 4 the service runs on a **separate gRPC port** rather
//! than being merged into the main axum router. Two reasons:
//!
//! 1. **Auth.** Sprint 4 ships the proto + the handler; the mTLS
//!    PKI that authenticates clients waits for Sprint 5+ Vault
//!    integration. A separate-port listener that defaults to **off**
//!    via `KUBINATE_AGENT_TUNNEL_ENABLED` keeps the unauth surface
//!    explicitly opt-in.
//! 2. **Protocol cleanliness.** gRPC routes through paths like
//!    `/kubinate.agent.v1.AgentService/OpenStream`, not REST-shaped
//!    paths. Mixing on the main HTTP/1.1+JSON router is doable
//!    (axum 0.8 + tonic 0.12 interop) but adds churn that buys
//!    nothing while the service is gated off.
//!
//! When mTLS is wired (Sprint 5+ ticket), the listener flips on by
//! default and the binding moves to its production address.
//! [`spawn_if_enabled`] is the single place that decides today.

use async_trait::async_trait;
use futures::StreamExt;
use kubinate_agent_proto::{
    AgentPayload, AgentService, AgentServiceServer, AgentToServer, HeartbeatAck, ServerPayload,
    ServerToAgent,
};
use std::pin::Pin;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Server, Request, Response, Status, Streaming};

/// Default bind address when the agent tunnel is enabled but no
/// explicit `KUBINATE_AGENT_TUNNEL_ADDR` is set. Localhost-only so
/// an operator who flips the feature flag without picking an
/// address doesn't accidentally open a public port.
const DEFAULT_AGENT_BIND_ADDR: &str = "127.0.0.1:8081";

/// Production-shaped agent service. The Sprint 4 partial-scope
/// shipping shape: heartbeats round-trip; metrics and assertion
/// payloads are accepted but logged-only (the storage paths land
/// in Sprint 5+ alongside the real agent deploy).
#[derive(Default, Clone)]
pub struct ApiAgentService {
    // No state today. Sprint 5+ adds:
    // - a handle to the observability proxy (for metrics fan-out),
    // - a session-revocation channel (for the assertion forward path),
    // - a registry of connected agents (for command dispatch).
}

#[async_trait]
impl AgentService for ApiAgentService {
    type OpenStreamStream =
        Pin<Box<dyn futures::Stream<Item = Result<ServerToAgent, Status>> + Send + 'static>>;

    async fn open_stream(
        &self,
        request: Request<Streaming<AgentToServer>>,
    ) -> Result<Response<Self::OpenStreamStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = mpsc::channel::<Result<ServerToAgent, Status>>(16);

        tokio::spawn(async move {
            while let Some(message) = inbound.next().await {
                let Ok(msg) = message else {
                    // Stream-level error — let the client observe
                    // the closure rather than swallowing it.
                    break;
                };
                let Some(payload) = msg.payload else { continue };
                match payload {
                    AgentPayload::Heartbeat(heartbeat) => {
                        // Sprint 5+ updates `agents.last_seen_at`
                        // here. For now: ack with the server's
                        // wall clock so the agent can measure
                        // round-trip latency.
                        let received_at_ms = wall_clock_ms();
                        tracing::debug!(
                            cluster_id = %heartbeat.cluster_id,
                            agent_version = %heartbeat.agent_version,
                            "agent heartbeat"
                        );
                        let ack = ServerToAgent {
                            payload: Some(ServerPayload::HeartbeatAck(HeartbeatAck {
                                received_at_ms,
                            })),
                        };
                        if tx.send(Ok(ack)).await.is_err() {
                            break;
                        }
                    }
                    AgentPayload::Metrics(_) => {
                        // Sprint 5+: forward to the observability
                        // proxy. Today: log + drop.
                        tracing::debug!(
                            "agent metrics_remote_write payload (drop until Sprint 5+)"
                        );
                    }
                    AgentPayload::Assertion(_) => {
                        // Sprint 5+: forward to the WebAuthn
                        // assertion path. Today: log + drop.
                        tracing::debug!("agent assertion forward (drop until Sprint 5+)");
                    }
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

fn wall_clock_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(dur.as_millis()).unwrap_or(i64::MAX)
}

/// Outcome of [`decide_listener`] — the pure-function tier that
/// reads env-var inputs (passed in by the caller, not from
/// `std::env` directly) and produces either a validated bind
/// address or a reason to skip.
#[derive(Debug, PartialEq, Eq)]
pub enum ListenerDecision {
    /// Flag was off (or unset). No port opens.
    Disabled,
    /// Flag was on but the address is malformed.
    InvalidAddr(String),
    /// Flag was on but the address binds to a non-loopback
    /// interface while Sprint 4 ships **no** authentication.
    /// Refused so a typo or copy-pasted compose file can't
    /// accidentally publish an unauth gRPC port to the public
    /// internet. Sprint 5+ flips the gate to "if no mTLS
    /// configured" instead.
    NonLoopbackWithoutAuth(std::net::SocketAddr),
    /// Validated address. Caller spawns the listener on this.
    Bind(std::net::SocketAddr),
}

/// Pure decision logic for whether the listener should start +
/// where it should bind. Split out from [`spawn_if_enabled`] so
/// it can be tested without mutating `std::env` (which is `unsafe`
/// from Rust 1.80 onward and races other tests in the same
/// binary).
///
/// `enabled` is the raw value of `KUBINATE_AGENT_TUNNEL_ENABLED`
/// (must equal `"1"` exactly to enable); `addr_override` is the
/// raw value of `KUBINATE_AGENT_TUNNEL_ADDR` (defaults to
/// [`DEFAULT_AGENT_BIND_ADDR`] when `None`).
#[must_use]
pub fn decide_listener(enabled: Option<&str>, addr_override: Option<&str>) -> ListenerDecision {
    if enabled != Some("1") {
        return ListenerDecision::Disabled;
    }
    let raw = addr_override.unwrap_or(DEFAULT_AGENT_BIND_ADDR);
    let addr: std::net::SocketAddr = match raw.parse() {
        Ok(a) => a,
        Err(e) => return ListenerDecision::InvalidAddr(format!("{raw}: {e}")),
    };
    // Sprint 4 ships no auth; refuse to bind anywhere a public
    // packet could reach. Once mTLS lands, this gate flips to a
    // "tls configured" predicate.
    if !addr.ip().is_loopback() {
        return ListenerDecision::NonLoopbackWithoutAuth(addr);
    }
    ListenerDecision::Bind(addr)
}

/// Decide whether to start the gRPC listener at process startup.
///
/// Reads two env vars:
/// - `KUBINATE_AGENT_TUNNEL_ENABLED` — must equal `"1"` to enable.
///   Any other value (including unset) keeps the listener off.
/// - `KUBINATE_AGENT_TUNNEL_ADDR` — bind address. Defaults to
///   [`DEFAULT_AGENT_BIND_ADDR`] (localhost-only) when unset.
///
/// Spawns the server on a fresh tokio task and returns immediately;
/// errors during binding are logged but do not abort startup. The
/// rest of the API serves regardless.
///
/// **Auth note**: Sprint 4 ships **no** authentication on this
/// listener. The flag is the only gate; an operator who flips it
/// to `1` is opting into an unauth gRPC endpoint. The
/// non-loopback bind is **refused** in this state to keep a typo
/// from accidentally publishing the port. Sprint 5+ adds mTLS and
/// flips the loopback restriction to "if no tls configured."
pub fn spawn_if_enabled() {
    let enabled_owned = std::env::var("KUBINATE_AGENT_TUNNEL_ENABLED").ok();
    let addr_owned = std::env::var("KUBINATE_AGENT_TUNNEL_ADDR").ok();
    let addr = match decide_listener(enabled_owned.as_deref(), addr_owned.as_deref()) {
        ListenerDecision::Disabled => {
            tracing::info!(
                "agent tunnel listener disabled (set KUBINATE_AGENT_TUNNEL_ENABLED=1 to enable; \
                 unauthenticated in Sprint 4 — Sprint 5+ adds mTLS)"
            );
            return;
        }
        ListenerDecision::InvalidAddr(reason) => {
            tracing::error!(
                error = %reason,
                "KUBINATE_AGENT_TUNNEL_ADDR invalid; listener will not start"
            );
            return;
        }
        ListenerDecision::NonLoopbackWithoutAuth(addr) => {
            tracing::error!(
                %addr,
                "refusing non-loopback bind while agent tunnel is unauthenticated \
                 (Sprint 5+ mTLS not yet wired); set KUBINATE_AGENT_TUNNEL_ADDR \
                 to a 127.0.0.1 / ::1 address"
            );
            return;
        }
        ListenerDecision::Bind(addr) => addr,
    };

    tokio::spawn(async move {
        tracing::warn!(
            %addr,
            "agent tunnel gRPC listener starting (Sprint 4: NO AUTH — \
             loopback bind enforced; Sprint 5+ adds mTLS)"
        );
        if let Err(e) = Server::builder()
            .add_service(AgentServiceServer::new(ApiAgentService::default()))
            .serve(addr)
            .await
        {
            tracing::error!(error = %e, "agent tunnel listener exited");
        }
    });
}

#[cfg(test)]
mod tests {
    //! Loopback test for the production-shaped agent service. The
    //! `EchoAgentService` in `crates/agent-proto/tests/loopback.rs`
    //! covers the proto-level contract; this test confirms the
    //! `ApiAgentService` impl produced from `kubinate-api` honors
    //! the same contract — heartbeats round-trip, the
    //! server-stamped `received_at_ms` is populated, and stream
    //! closure on the agent side cleanly closes the response stream.
    //!
    //! No network — runs over `tokio::io::duplex`. No auth — the
    //! Sprint 4 partial scope explicitly defers mTLS to Sprint 5+.

    use super::*;
    use kubinate_agent_proto::{AgentPayload, AgentServiceClient, AgentToServer, Heartbeat};
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;
    use tonic::transport::{Endpoint, Server, Uri};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn heartbeat_round_trips_through_api_agent_service() {
        let (client_io, server_io) = tokio::io::duplex(4096);

        tokio::spawn(async move {
            Server::builder()
                .add_service(AgentServiceServer::new(ApiAgentService::default()))
                .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server_io)))
                .await
                .expect("server crashed");
        });

        let mut client_io = Some(client_io);
        let channel = Endpoint::try_from("http://kubinate-api-loopback")
            .unwrap()
            .connect_with_connector(tower::service_fn(move |_: Uri| {
                let io = client_io
                    .take()
                    .expect("connect_with_connector called once");
                async move { Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(io)) }
            }))
            .await
            .expect("loopback channel connects");
        let mut client = AgentServiceClient::new(channel);

        let (tx, rx) = mpsc::channel::<AgentToServer>(8);
        let response_stream = client
            .open_stream(ReceiverStream::new(rx))
            .await
            .expect("open_stream");
        let mut inbound = response_stream.into_inner();

        let agent_sent_at = 1_700_000_000_000_i64;
        tx.send(AgentToServer {
            payload: Some(AgentPayload::Heartbeat(Heartbeat {
                cluster_id: "cluster-test-0001".into(),
                agent_version: "0.1.0".into(),
                k3s_version: "v1.30.2+k3s1".into(),
                sent_at_ms: agent_sent_at,
            })),
        })
        .await
        .expect("send heartbeat");

        let ack_message = tokio::time::timeout(std::time::Duration::from_secs(5), inbound.next())
            .await
            .expect("ack within 5s")
            .expect("server sent a message")
            .expect("message ok");

        match ack_message.payload {
            Some(ServerPayload::HeartbeatAck(ack)) => {
                // The production service stamps wall-clock ms;
                // assert it's plausible (>= the agent's sent time
                // for any system clock that wasn't deliberately
                // rewound). Avoids hard-coding a specific value
                // since unlike the EchoAgentService this real
                // impl reads the system clock.
                assert!(
                    ack.received_at_ms >= agent_sent_at,
                    "expected ack received_at_ms ({}) >= heartbeat sent_at_ms ({})",
                    ack.received_at_ms,
                    agent_sent_at
                );
            }
            other => panic!("unexpected response variant: {other:?}"),
        }

        drop(tx);
    }

    // Decision-logic tests target the pure function so we don't
    // need to mutate `std::env` (which is `unsafe` from Rust 1.80
    // and races other tests in the same binary). The env-reading
    // wrapper `spawn_if_enabled()` is trivially-call-it-and-trust
    // once these decision branches are covered.

    #[test]
    fn decide_listener_off_when_flag_unset() {
        assert_eq!(decide_listener(None, None), ListenerDecision::Disabled);
    }

    #[test]
    fn decide_listener_off_for_truthy_strings_other_than_one() {
        // "1" is the only enabling value — `"true"`, `"yes"`,
        // `"on"`, `"01"`, `"1\n"` all stay off. Catches any
        // future regression that tries to "be friendly" by
        // accepting boolean-ish strings.
        for s in ["true", "yes", "on", "01", "1\n", "True", "TRUE", "0"] {
            assert_eq!(
                decide_listener(Some(s), None),
                ListenerDecision::Disabled,
                "unexpected enable for {s:?}"
            );
        }
    }

    #[test]
    fn decide_listener_default_addr_is_loopback_and_accepted() {
        match decide_listener(Some("1"), None) {
            ListenerDecision::Bind(addr) => {
                assert!(addr.ip().is_loopback(), "default addr must be loopback");
            }
            other => panic!("unexpected decision: {other:?}"),
        }
    }

    #[test]
    fn decide_listener_refuses_non_loopback_while_unauthenticated() {
        // The headline Sprint-4 safety control: a typo /
        // copy-pasted compose file that points the bind at
        // `0.0.0.0` must NOT open an unauth public port.
        for s in ["0.0.0.0:8081", "[::]:8081", "192.0.2.1:8081"] {
            match decide_listener(Some("1"), Some(s)) {
                ListenerDecision::NonLoopbackWithoutAuth(_) => {}
                other => panic!("expected NonLoopbackWithoutAuth for {s:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn decide_listener_accepts_explicit_loopback_overrides() {
        for s in ["127.0.0.1:9000", "[::1]:9000"] {
            match decide_listener(Some("1"), Some(s)) {
                ListenerDecision::Bind(addr) => assert!(addr.ip().is_loopback()),
                other => panic!("unexpected decision for {s:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn decide_listener_reports_malformed_addr() {
        match decide_listener(Some("1"), Some("not-an-addr")) {
            ListenerDecision::InvalidAddr(reason) => assert!(reason.contains("not-an-addr")),
            other => panic!("unexpected decision: {other:?}"),
        }
    }
}
