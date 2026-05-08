//! Sprint 4 ticket 03 — gRPC service definitions for the agent
//! reverse tunnel.
//!
//! The protobuf source lives in `proto/agent.proto`; `build.rs`
//! runs `tonic-build` at compile time to emit the trait + message
//! types under `OUT_DIR`. This module re-exports the generated code
//! at a stable path so consumers (the agent binary at the workspace
//! root, the API control-plane endpoint) can write
//! `kubinate_agent_proto::agent_service_server::AgentService`
//! without depending on the protoc include path.
//!
//! Wire shape, in one paragraph: the agent dials the control plane,
//! presents its mTLS client certificate, and opens a single
//! bidirectional gRPC stream. Both sides multiplex messages over
//! the stream — `AgentToServer` going up (heartbeats, metrics
//! remote-write, assertion results), `ServerToAgent` coming back
//! (heartbeat acks, occasional commands). The control plane never
//! dials toward the customer cluster (CLAUDE.md inbound-only
//! invariant).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Generated tonic + prost code. Re-exported here so consumers can
/// say `use kubinate_agent_proto::v1::*` rather than juggling the
/// build-script include path.
///
/// `missing_docs` is suppressed locally because tonic-build /
/// prost-derive emit unannotated structs + enums and re-running
/// codegen with our doc strings would require maintaining a wrapper
/// per generated type.
#[allow(missing_docs)]
pub mod v1 {
    // Output filename mirrors the proto package: `kubinate.agent.v1`.
    tonic::include_proto!("kubinate.agent.v1");
}

pub use v1::agent_service_client::AgentServiceClient;
pub use v1::agent_service_server::{AgentService, AgentServiceServer};
pub use v1::{
    agent_to_server::Payload as AgentPayload, command::Kind as CommandKind,
    server_to_agent::Payload as ServerPayload, AgentToServer, AssertionResult, Command,
    GracefulRestart, Heartbeat, HeartbeatAck, MetricsRemoteWrite, ServerToAgent,
};
