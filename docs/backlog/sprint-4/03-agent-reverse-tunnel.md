# Ship the agent's mTLS gRPC reverse tunnel

**Labels**: `area/agent`, `area/security`, `sprint-4`
**Epic**: Phase 3 dogfood enablement
**Size**: 13 (high-confidence; protocol design + workspace
build of the agent's gRPC stack + control-plane endpoint +
deployment story)

## Context

The Phase-2 agent at
[`agent/src/main.rs`](../../../agent/src/main.rs) is a stub that
logs and exits. CLAUDE.md commits to the production shape:

> agent/ — in-cluster static binary `kubinate-agent`. Opens an
> agent-initiated mTLS gRPC reverse tunnel to the control plane;
> user clusters do not accept inbound connections from the
> control plane.

The roadmap names "Per-cluster agent reverse tunnel shipped" as
a Phase 3 exit gate (`docs/roadmap.md`). It's also the path the
production observability proxy (Sprint 4 ticket 04) uses to ship
metrics back, and the path future out-of-band command flows
(force-rotate, in-cluster `kubectl get`) will use.

## When this opens

After the dogfood-cluster ticket lands (the agent runs on the
dogfooded k3s first as the migration's smoke test). Should land
**before** ticket 04 (production observability proxy), because
the proxy's remote-write path is the agent's first real consumer.
Vault (ticket 02) is a **soft prerequisite**: the agent's
short-lived per-cluster credentials want to be Vault-issued JWTs,
not pgcrypto blobs handed out at provision time.

## Sprint 4 partial scope (~5 of 13 pts)

Sprint 4 ships the protocol + the in-process loopback test;
the production deploy waits for the dogfood cluster + Vault PKI:

- [x in Sprint 4] New crate `crates/agent-proto/` with the
  `tonic` gRPC service definitions (`Heartbeat`,
  `MetricsRemoteWrite`, the bidirectional stream shape).
- [x in Sprint 4] Control-plane endpoint at
  `/v1/agents/connect` that **accepts** the bidirectional
  stream + dispatches RPCs (no auth check yet beyond a
  feature-flag gate that defaults off).
- [x in Sprint 4] In-process loopback test: control plane +
  agent in the same Tokio runtime round-trip a `Heartbeat`
  and a synthetic `metrics_remote_write` call.

Deferred to Sprint 5+ (when Vault PKI + the dogfood deploy
exist):

- [ ] mTLS handshake against a real CA.
- [ ] Per-cluster client cert issued by Vault PKI (depends on
      ticket 02 going to production).
- [ ] The agent binary (replacing the stub at
      `agent/src/main.rs`) deployed via cloud-init.
- [ ] The `agent-heartbeat-missing.md` runbook updated with
      live diagnostic commands.
- [ ] All five rows of the **Security DoD** section below.
      None of them can be honestly checked off without the
      live mTLS path.

The Sprint 4 close ticks **only** the protocol-shipping rows
above. The Security DoD is gated on the deferred items
landing first — opening this ticket without a security
review of the wire shape is the right move; closing it
without the security review of the live mTLS handshake is
not.

## Acceptance criteria

- **Given** a fresh user cluster, **when** `kubinate-agent` boots
  via the cloud-init that already runs on every node (Sprint 1
  ticket 03), **then** it dials the control plane on
  `wss://api.kubinate.example/v1/agents` (or a dedicated
  `agent.kubinate.example` host — decide at design time), presents
  its mTLS client certificate, and holds the connection open.
- **Given** an open tunnel, **when** the control plane needs to
  invoke an in-cluster operation (e.g. `metrics_remote_write`),
  **then** it sends the gRPC call **back** through the tunnel; the
  agent executes locally and streams the response back. **No
  inbound connection** ever opens against the user cluster.
- **Given** the tunnel drops (network blip, agent restart),
  **when** the agent restarts, **then** it reconnects with an
  exponential backoff capped at 60s; the control plane treats the
  reconnect as the same logical agent (matching cert serial +
  cluster id).
- **Given** an attacker tries to dial the agent endpoint with a
  cert minted by a different CA, **when** the connection is
  attempted, **then** the control plane rejects at the TLS layer
  before any gRPC call is dispatched. (Mutual auth — the agent
  also validates the control plane's server cert.)

## Implementation notes

- **Crate split**: today's `agent/` is a binary at the workspace
  root. Phase 3 keeps that, but moves the gRPC service definitions
  into a new `crates/agent-proto/` crate (workspace-shared between
  the agent binary and the control-plane endpoint). Use `tonic`
  (already a workspace dep).
- **mTLS PKI**: a dedicated CA is provisioned during cluster
  bootstrap; per-cluster client certs are issued by the API at
  cluster-create time and stored via the `SecretStore` trait. With
  Vault landed (ticket 02), this becomes Vault PKI; with pgcrypto
  it's a self-rolled OpenSSL flow. **Pick one path**, don't ship
  both — block this ticket on Vault if necessary.
- **Reverse-tunnel mechanic**: the agent maintains a single
  bidirectional gRPC stream; the control plane dispatches RPCs
  as messages on that stream. (Same shape as
  `tonic`-style server streaming, but with the agent as the
  "server" of further requests after dialling out.)
- **Heartbeat**: the agent sends a periodic
  `agent.v1.Heartbeat{cluster_id, agent_version, k3s_version}`.
  The control plane's `agent-heartbeat-missing.md` runbook is
  already drafted; this ticket actually wires the alert source
  it references.
- **Rate-limits + auth log**: every dispatched RPC lands in a
  new `agent_audit` table — same shape as the existing audit
  chain (ADR-0006), keyed on `cluster_id`. The runbook's
  diagnosis section already promises this exists.

## Acceptance / DoD

- [ ] `crates/agent-proto/` defines the gRPC service.
- [ ] `agent/` (the binary) replaces its stub `main.rs` with the
      real connect+dispatch loop, mTLS-authenticated, with the
      reconnect-backoff path tested via a wiremock-style harness.
- [ ] Control-plane endpoint at `/v1/agents/connect` that
      accepts the bidirectional stream + dispatches RPCs.
- [ ] Each tunnel drop fires a structured `tracing` event the
      `agent-heartbeat-missing.md` runbook can rely on.
- [ ] Integration test: in-process loopback (control plane +
      agent in the same test) round-trips a `Heartbeat` and a
      `metrics_remote_write` synthetic call.
- [ ] An ADR (0013 or next free) records the wire format
      decision (gRPC over HTTP/2 vs. raw WebSocket) + the cert
      issuance flow chosen. The decision-doc starter at
      `docs/decisions/sprint-3-observability-tsdb.md` references
      this; that reference becomes a real link.
- [ ] `docs/runbooks/agent-heartbeat-missing.md` updated with
      the actual diagnostic commands (today it points at fields
      that don't exist yet).
- [ ] No regression: every existing API endpoint still works —
      this ticket adds an endpoint; it doesn't change the existing
      ones.

## Security DoD (CONTRIBUTING.md `security`-role review required)

- [ ] mTLS handshake actually rejects unknown CAs (negative test).
- [ ] Per-cluster client cert is bound to the cluster id at
      issuance; a stolen cert from cluster A cannot impersonate
      cluster B even if presented to the control plane (the
      control plane verifies the cert's CN/SAN against the
      tunnelling cluster_id claim).
- [ ] No control-plane-initiated outbound to the user cluster
      (the inbound-only invariant from CLAUDE.md). Verified by
      a test that asserts no `dial` syscall goes to the user
      cluster's IP.

## What this ticket deliberately does **not** do

- **CRD-shaped command authorisation.** Phase 3+ ticket: a
  policy layer that says "this org's agent can run RPC X but
  not RPC Y." Until then, a hand-coded allowlist gates the
  RPC catalog.
- **Per-RPC tracing all the way down to the in-cluster call.**
  W3C trace context propagation across the tunnel is a
  follow-up; for the first ship, propagate `request_id` only.
