# ADR-0014: Adopt gRPC bidirectional streaming over mTLS for the agent reverse tunnel

- **Status**: Proposed
- **Date**: 2026-05-08
- **Deciders**: agent team, security maintainer
- **Tags**: area/agent, area/security, area/api, sprint-4

## Context

CLAUDE.md commits to a specific shape for the per-cluster agent:

> agent/ — in-cluster static binary `kubinate-agent`. Opens an
> agent-initiated mTLS gRPC reverse tunnel to the control plane;
> user clusters do not accept inbound connections from the
> control plane.

The "inbound-only" invariant is load-bearing for two reasons. First,
customer clusters live on networks we don't control — many sit behind
NAT, corporate firewalls, or split-horizon DNS. Requiring inbound
reachability on the user side would force an operations burden onto
every customer (port-forward, public IP, etc.) and is a non-starter
for the dogfood plan. Second, the security threat model
(`docs/security/threat-model.md`) treats the user cluster as a
lower-trust zone than the control plane: if a customer's worker node
is compromised, the blast radius must not include "open new
connections into the control plane on its behalf." A *reverse* tunnel
where the agent is the only side that initiates, authenticated by an
mTLS client cert bound to the cluster id, makes that property fall
out of the protocol rather than relying on application-level checks.

Sprint 4 ticket 03 ships the protocol shape (proto definitions in
`crates/agent-proto`, control-plane handler in `crates/api/src/agent.rs`,
in-process loopback test). The mTLS handshake against a real CA, the
per-cluster cert issuance flow, and the binary that replaces the
stub at `agent/src/main.rs` are deferred to Sprint 5+, gated on
Vault PKI (Sprint 4 ticket 02) and the dogfood-cluster migration
(Sprint 4 ticket 06). This ADR records the wire-format decision now,
before the deferred work lands, so the security review of the wire
shape can happen in parallel with the Vault work — closing the
ticket without a security review of the live mTLS handshake is not
something this ADR licenses.

The decision space is narrow but real. The two mainstream options
for "single long-lived connection from agent to control plane,
multiplexed for heartbeat + metrics + commands" are:

1. **gRPC over HTTP/2 with a bidirectional streaming RPC** —
   the agent calls `OpenStream(stream AgentToServer)` returning
   `(stream ServerToAgent)`. Both sides multiplex `oneof`-tagged
   messages over the stream. The control plane sends "reverse"
   RPCs to the agent as `ServerToAgent.Command` messages.

2. **Raw WebSocket with a hand-rolled framing layer** — the agent
   opens a `wss://` connection, both sides exchange JSON or CBOR
   frames, and the framing carries an envelope (request id,
   payload type, etc.) that an application-level dispatcher
   demultiplexes.

Both satisfy the inbound-only invariant. The differences are at the
ergonomics, ecosystem, and schema-evolution layer.

## Decision

We will use **gRPC bidirectional streaming over HTTP/2 with mTLS**
for the agent reverse tunnel, with the wire format already shipped
in `crates/agent-proto/proto/agent.proto`. Per-cluster client certs
will be issued by **Vault PKI** at cluster-create time, stored via
the existing `SecretStore` trait (ADR-0007), and rotated by the
graceful-restart command path baked into the proto.

Concretely:

- **Transport**: HTTPS/2 with mutual TLS. The control plane presents
  a certificate signed by Kubinate's public CA; the agent presents a
  per-cluster client cert signed by a **dedicated agent CA** (one CA
  per environment — staging, production — *not* one CA per cluster).
  Both sides verify the peer's full chain.

- **Cert binding**: the agent client cert's CN is the cluster id
  (UUID v7), the SAN is `cluster.<cluster_id>.agents.kubinate.internal`.
  The control plane's gRPC interceptor extracts the verified cert
  CN, looks up the cluster row, and stamps the resolved
  `(organization_id, cluster_id)` onto the request extensions.
  Application handlers cross-check any `cluster_id` field in the
  payload against this stamped value — a body claiming a different
  cluster id is rejected with `tenant_mismatch`, mirroring the same
  guard the JSON observability proxy already enforces. **A stolen
  cert from cluster A cannot be presented as cluster B even with
  control of the body bytes.**

- **Cert issuance**: Vault PKI signs each per-cluster cert at
  cluster-create time with a 30-day validity. The cert + private
  key are stored via `SecretStore::put` (encrypted at rest by the
  existing pgcrypto / Vault transit path) and embedded in the
  cluster's cloud-init payload so the agent boots with them. The
  graceful-restart command lets the control plane trigger a
  re-issuance + rotation cycle without operator intervention.

- **Reconnect**: the agent reconnects with exponential backoff
  starting at 1s and capped at 60s, with full jitter. The control
  plane keys connected agents on `(cluster_id, cert_serial)` —
  matching identifiers across a reconnect mean "same logical agent,"
  even if the underlying TCP connection is new.

- **Multiplexing**: a single bidi stream carries all message kinds.
  `oneof AgentToServer.payload` covers `Heartbeat`,
  `MetricsRemoteWrite`, and `AssertionResult`; future kinds add new
  variants without breaking older agent binaries (proto3 unknown-
  variant skip semantics). The control plane's reverse-RPC channel
  is `ServerToAgent.Command`, also a `oneof`, today carrying only
  `GracefulRestart`.

The Sprint 4 partial scope (proto + control-plane handler +
loopback test) is the ratification of this decision at the wire
level. The ratification is **conditional** on the Sprint 5+
follow-ups (real CA, Vault PKI flow, agent binary deploy) landing
before the listener flips on outside of the in-process loopback;
`KUBINATE_AGENT_TUNNEL_ENABLED` defaults off and the production
deploy story names mTLS enforcement as the gating signal.

## Alternatives considered

**Raw WebSocket with a hand-rolled framing layer.** Rejected on
ecosystem and schema-evolution grounds. tonic + prost give us a
binary wire format with field numbers, unknown-field skip, and a
generated client/server pair *for free*; rolling our own envelope
re-creates that infrastructure with worse tooling (no `prost-build`,
no `cargo expand` introspection, no language-portable
`.proto` source of truth for any future SDK). The stake of WebSocket
as transport — application/binary framing on top of HTTP/1.1 Upgrade —
also forfeits HTTP/2's per-stream flow control, which we get for
nothing on gRPC. The "we already write JSON over `Sec-WebSocket-Protocol`
elsewhere" argument doesn't apply: the dashboard's SSE `events` channel
is a one-way HTTP/1.1 stream, not bidirectional, and shares no
implementation with this layer.

**gRPC unary RPCs with the agent polling.** Rejected because polling
inverts the latency story: heartbeat-driven liveness becomes
poll-interval-bounded, metrics buffer locally on the agent until the
next poll, and the control plane can no longer push a `GracefulRestart`
command without waiting for the agent to come ask. The bidi stream
is the simplest shape that lets either side speak first.

**HTTP/2 server push for the reverse direction.** Rejected because
server push is being deprecated by browsers and is not a stable
target across HTTP/2 implementations; tonic doesn't expose it; and
the bidi stream covers the same use case with fewer moving parts.

**Skip mTLS, use bearer tokens over TLS.** Rejected on the
threat-model grounds the ticket already names. A bearer token
leaked from one cluster's filesystem (k3s logs, env vars, a stray
`kubectl describe pod`) could be replayed from anywhere on the
internet to impersonate that cluster. mTLS pins the auth to *cert
+ private key together*; lifting the cert without the key is
useless, and the key never leaves the cluster's encrypted disk.
The operational tax of issuing per-cluster certs is real but is the
same tax we already pay for the kubeconfig handed to operators —
this ticket reuses the issuance plumbing.

**One CA per cluster (instead of one CA per environment).**
Rejected. Cert-chain depth-of-2 (env-CA → per-cluster cert) is the
sweet spot: leaking a per-cluster *cert* compromises one cluster
(short-lived, rotated every 30 days); leaking the *env CA* is a
P0 incident regardless of how many CAs we have. A per-cluster CA
only narrows the second case at a steep operational cost (every
new cluster spawns a CA in Vault), and Vault's PKI engine doesn't
optimize for CA-cardinality at that scale.

## Consequences

**We accept** that the agent stack pulls in `tonic`, `prost`, and
HTTP/2 — already workspace dependencies, so no new compile-time
cost. The agent binary's static link is correspondingly larger
than a hand-rolled WebSocket client would be (~ 6 MB vs. ~ 2 MB
for a comparable WebSocket-only stack); the trade-off is bought
back by not maintaining a custom framing layer.

**We accept** that the listener has to live on a separate gRPC
port (`KUBINATE_AGENT_TUNNEL_ADDR`, default `127.0.0.1:8081`)
during the Sprint 4 partial-scope window. axum 0.8 + tonic 0.12
interop on a single port is doable but adds churn that buys
nothing while the service is gated off; the production address
binding moves with the mTLS-enforcement flip in Sprint 5+.

**We assume** Vault transit / PKI lands in production by Sprint 5
(ticket 02). The fallback is a self-rolled OpenSSL flow with the
same wire-level cert shape — same CN, same SAN format, same chain
verification — so the wire format decided here doesn't reopen if
the Vault dependency slips. The fallback is operationally heavier
(manual rotation), not architecturally different.

**We assume** HTTP/2 traversal across customer firewalls is
ubiquitous enough not to be a deployment blocker. Standard HTTPS
egress on 443 covers the dogfood case and every cloud customer we
expect in Phase 3; HTTP/2 specifically (not 1.1) does sometimes get
mangled by middleboxes that proxy-terminate TLS, in which case the
agent error-loops with a clear connection-reset signal and the
operator routes around the box. This trigger is named in the
revisit section.

**We force** a structured `tracing` event on every tunnel
disconnect, keyed on `cluster_id` and `cert_serial`. The runbook
at `docs/runbooks/agent-heartbeat-missing.md` references this
event in its diagnosis section; the field names land in
`crates/api/src/agent.rs` as part of the connection-lifecycle
instrumentation, not free text.

**We force** the audit chain to extend to a new `agent_audit`
table — same shape as the existing chain (ADR-0006), keyed on
`cluster_id`. Every dispatched `Command` (today only
`GracefulRestart`; tomorrow whatever Phase 3+ adds) lands a row
before the message goes on the wire. Without this row a compromised
control-plane process could issue restart commands without leaving
a trail; the audit row is what makes the post-incident answer to
"what did the control plane tell each cluster's agent?" honest.

## Revisit trigger

Reopen this ADR if **any** of:

- More than 5% of attempted agent connections fail at the HTTP/2
  layer in steady-state (measured by the structured `tracing` event
  named above). That signal means HTTP/2 traversal across customer
  firewalls is not the no-op assumed here, and the wire-format
  question reopens with WebSocket as the realistic alternative.
- Vault PKI is *not* in production by the start of Sprint 6, AND
  the self-rolled OpenSSL fallback is operationally insolvent
  (e.g. manual rotation can't keep up with cluster cardinality).
  In that case the cert-issuance section of this ADR reopens —
  the wire shape stays.
- A new RPC kind requires a *non-streamable* return shape (e.g. a
  large blob upload from the control plane to the agent). Today's
  `oneof` Command variants are bounded; a multi-megabyte payload
  would push us into chunked file transfer that doesn't fit cleanly
  in a `oneof`. Address by adding a sibling unary RPC alongside the
  bidi stream, *not* by changing the bidi shape.

A revisit is **not** triggered by adding new `Command` or
`AgentToServer` payload variants — that's exactly what proto3
schema evolution is for, and ADR ratification per-variant would
ossify the wire format unhelpfully.

## References

- CLAUDE.md (`agent/` paragraph + `Load-bearing invariants` row 1
  on tenant isolation).
- ADR-0006 — multi-tenancy + RLS, the audit chain pattern this
  ADR extends to `agent_audit`.
- ADR-0007 — secret management, the `SecretStore` trait used to
  store the per-cluster cert + private key.
- `docs/backlog/sprint-4/03-agent-reverse-tunnel.md` — the parent
  ticket, including the Security DoD rows this ADR's revisit
  section is paired with.
- `docs/runbooks/agent-heartbeat-missing.md` — the runbook whose
  diagnosis section this ADR's tracing-event commitment binds to.
- `crates/agent-proto/proto/agent.proto` — the wire format this
  ADR ratifies.
- `crates/api/src/agent.rs` — the control-plane handler that
  consumes the wire format.
- [Why bidi streaming for agent reverse tunnels — gRPC docs](https://grpc.io/docs/what-is-grpc/core-concepts/#bidirectional-streaming-rpc)
