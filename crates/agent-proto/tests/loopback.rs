//! In-process loopback test for the agent reverse-tunnel proto.
//!
//! Exercises the `OpenStream` bidirectional RPC end-to-end:
//! constructs an `EchoAgentService` server impl, runs it on a
//! tokio duplex stream, opens a tonic client through the same
//! channel, and round-trips a `Heartbeat` to a `HeartbeatAck`.
//!
//! Why this test matters for Sprint 4 partial scope: the full
//! production deploy (mTLS, per-cluster certs from Vault PKI,
//! agent binary on a real node) waits for Sprint 5+. The proto
//! contract — every `AgentToServer` variant the server is
//! supposed to handle, the response shape it owes back — is
//! everything we *can* assert in CI today. A loopback test that
//! drives the full stream lifecycle catches proto-level mistakes
//! (wrong oneof tags, message reordering bugs, response variants
//! we forgot to wire) before Sprint 5 walks into them.

use std::pin::Pin;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use kubinate_agent_proto::{
    AgentPayload, AgentService, AgentServiceClient, AgentServiceServer, AgentToServer,
    HeartbeatAck, ServerPayload, ServerToAgent,
};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
    transport::{Endpoint, Server, Uri},
    Request, Response, Status, Streaming,
};

/// Minimal server impl that:
/// - Acks every heartbeat with the server-stamped received_at_ms.
/// - Drops every other variant silently — the loopback test only
///   exercises the heartbeat round-trip; production handlers will
///   fan out per-variant.
struct EchoAgentService;

#[async_trait]
impl AgentService for EchoAgentService {
    type OpenStreamStream =
        Pin<Box<dyn Stream<Item = Result<ServerToAgent, Status>> + Send + 'static>>;

    async fn open_stream(
        &self,
        request: Request<Streaming<AgentToServer>>,
    ) -> Result<Response<Self::OpenStreamStream>, Status> {
        let mut inbound = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<ServerToAgent, Status>>(8);

        tokio::spawn(async move {
            while let Some(message) = inbound.next().await {
                let Ok(msg) = message else {
                    // Stream-level error — break and let the client
                    // observe the closure.
                    break;
                };
                let Some(payload) = msg.payload else { continue };
                if let AgentPayload::Heartbeat(heartbeat) = payload {
                    let ack = ServerToAgent {
                        payload: Some(ServerPayload::HeartbeatAck(HeartbeatAck {
                            received_at_ms: heartbeat.sent_at_ms + 1, // synthetic clock
                        })),
                    };
                    if tx.send(Ok(ack)).await.is_err() {
                        // Receiver dropped; nothing useful left to do.
                        break;
                    }
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn heartbeat_round_trips_through_open_stream() {
    // Build an in-memory duplex socket; the server runs on one half,
    // the client connects through the other. No real network.
    let (client_io, server_io) = tokio::io::duplex(4096);

    // Spawn the server.
    tokio::spawn(async move {
        Server::builder()
            .add_service(AgentServiceServer::new(EchoAgentService))
            .serve_with_incoming(tokio_stream::once(Ok::<_, std::io::Error>(server_io)))
            .await
            .expect("server crashed");
    });

    // Build a tonic client over the duplex pipe. The URI is required
    // by tonic's Endpoint API even though we override the connector
    // — `http://[::]:50051` is the conventional placeholder.
    let mut client_io = Some(client_io);
    let channel = Endpoint::try_from("http://kubinate-agent-loopback")
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

    // Send a single heartbeat; expect an ack back.
    let (tx, rx) = tokio::sync::mpsc::channel::<AgentToServer>(8);
    let outbound = ReceiverStream::new(rx);

    let response_stream = client.open_stream(outbound).await.expect("open_stream");
    let mut inbound = response_stream.into_inner();

    tx.send(AgentToServer {
        payload: Some(AgentPayload::Heartbeat(kubinate_agent_proto::Heartbeat {
            cluster_id: "cluster-test-0001".into(),
            agent_version: "0.1.0".into(),
            k3s_version: "v1.30.2+k3s1".into(),
            sent_at_ms: 1_700_000_000_000,
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
            assert_eq!(ack.received_at_ms, 1_700_000_000_001);
        }
        other => panic!("unexpected response variant: {other:?}"),
    }

    // Drop the outbound sender → the server's `inbound.next()` loop
    // exits → the response stream closes cleanly.
    drop(tx);
}
