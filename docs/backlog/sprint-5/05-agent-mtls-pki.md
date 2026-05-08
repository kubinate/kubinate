# Ship the agent reverse-tunnel mTLS PKI + binary deploy

**Labels**: `area/agent`, `area/security`, `sprint-5`
**Epic**: Phase 3 dogfood enablement
**Size**: 8 (medium-confidence; the wire shape is fixed by
ADR-0014, the unknowns are the cert-issuance integration and
the agent-binary cloud-init path)

## Context

Sprint 4 ticket 03 shipped the partial scope (proto crate
`crates/agent-proto/`, control-plane handler at
`crates/api/src/agent.rs`, in-process loopback test) and
[ADR-0014](../../adr/0014-agent-reverse-tunnel-wire-format.md)
ratified the wire shape. The deferred items — real CA, Vault
PKI cert issuance, agent-binary cloud-init, all five Security
DoD rows — gated on the dogfood cluster + Vault landing.
Sprint 5's #02 stands up the cluster; Sprint 5's #04 deploys
Vault. This ticket then closes the agent-tunnel work.

The five Security DoD rows of Sprint 4 ticket 03 are the
load-bearing acceptance criteria here. Without them, the
agent-tunnel feature is "built but not authentically secured"
— the SPA still works, but the inbound-only invariant from
CLAUDE.md is enforced only at the protocol layer, not at the
auth layer.

## Acceptance criteria

- **Given** the dogfood cluster and Vault are up, **when**
  the operator enables Vault's PKI secrets engine via the
  Sprint 5 Ansible role, **then** the engine signs a per-
  environment intermediate CA scoped to `agents-staging` /
  `agents-production`. The root CA is generated once and
  stored offline; the engine signs per-cluster client certs
  off the intermediate.
- **Given** a tenant requests a new cluster (existing
  Sprint 1 ticket 03 flow), **when** the cluster-create
  workflow's bootstrap activity runs, **then** it issues a
  per-cluster client cert via the Vault PKI engine with CN =
  `cluster.<cluster_id>` and SAN = `cluster.<cluster_id>.agents.kubinate.internal`.
  The cert + private key are stored via `SecretStore::put`
  (encrypted at rest by the Sprint 5-#04 Vault transit
  backend) and embedded in the cluster's cloud-init payload.
- **Given** a fresh user cluster's cloud-init runs,
  **when** k3s reaches Ready, **then** the new
  `kubinate-agent` binary boots from the static binary
  fetched at boot time, dials the control plane on
  `wss://agent.kubinate.example/v1/agents/connect` (or
  whatever ADR-0014's revisit picks), presents its mTLS
  client cert, and holds the connection open.
- **Given** an agent connection is open, **when** an
  attacker presents a cert minted by a different CA,
  **then** the control plane rejects at the TLS layer
  before any gRPC call is dispatched. Mutual auth — the
  agent verifies the control plane's server cert too.
- **Given** the agent's cert has 7 days left to expiry,
  **when** the cert-rotation workflow fires, **then** the
  control plane sends a `GracefulRestart` command on the
  open tunnel, the agent fetches a fresh cert via the
  bootstrap path, and the new cert binds the same
  cluster_id. The control plane treats the reconnect as the
  same logical agent.
- **Given** the tunnel drops (network blip, agent restart),
  **when** the agent restarts, **then** it reconnects with
  exponential backoff capped at 60s and full jitter. The
  control plane treats matching `(cluster_id, cert_serial)`
  as the same logical agent.
- **Given** the inbound-only invariant from CLAUDE.md,
  **when** the test suite runs the negative test, **then**
  no `dial` syscall in the control plane targets the user
  cluster's IP. (The control plane only ever responds on
  the bidi stream the agent opened.)

## Implementation notes

- **Crate split.** `crates/agent-proto/` already exists from
  Sprint 4. The agent binary lives at `agent/` (workspace
  root) and replaces its stub `main.rs`. New module
  `agent/src/connect.rs` for the connect+dispatch loop;
  `agent/src/cert.rs` for the cert-load + reload logic.
- **Cert storage on the agent side.** Embed the cert + key
  in the cloud-init `user-data` (templated server-side from
  the Vault PKI issuance result). The agent reads from
  `/etc/kubinate/agent.pem` + `/etc/kubinate/agent.key` at
  boot; the files are root-owned mode `0400`. On rotation
  the cloud-init is regenerated and the agent restarts via
  the `GracefulRestart` command — k3s service-restart
  semantics mean the new files are picked up.
- **Cert binding cross-check.** The control-plane gRPC
  interceptor extracts the verified cert CN, looks up the
  cluster row, and stamps the resolved
  `(organization_id, cluster_id)` onto the request
  extensions. Application handlers cross-check any
  `cluster_id` field in the payload against this stamped
  value — a body claiming a different cluster id is
  rejected with `tenant_mismatch`.
- **Reconnect backoff.** Standard exponential with
  full-jitter formula: `min(60s, base * 2^attempt) *
  random(0, 1)`. Reset attempt count on successful
  connection that holds for > 30s.
- **`agent_audit` table.** Every dispatched RPC lands a
  row keyed on `cluster_id`, mirroring the existing
  per-tenant audit chain shape (ADR-0006). The chain is
  per-cluster (not per-tenant) so the table needs its own
  trigger; reuse the `audit_log_append_explicit` plumbing
  pattern. Migration ships in this ticket.
- **Listener migration.** Today the listener runs on a
  separate gRPC port (`KUBINATE_AGENT_TUNNEL_ADDR`,
  default 127.0.0.1:8081) gated behind
  `KUBINATE_AGENT_TUNNEL_ENABLED`. With mTLS landed the
  listener moves to its production address (the
  ADR-0014-named host) and the env-var flag flips to
  default-on; the localhost-only fallback drops.

## DoD (mirrors Sprint 4 ticket 03 deferred + Security DoD)

- [ ] `agent/src/main.rs` replaces its stub with the real
      connect+dispatch loop, mTLS-authenticated, with the
      reconnect-backoff path tested via a wiremock-style
      harness.
- [ ] Vault PKI engine deployed (Sprint 5-#04's Ansible
      role gets a sibling that brings up the PKI engine);
      per-environment intermediate CA scoped correctly.
- [ ] Cluster-create workflow's bootstrap activity issues
      a per-cluster client cert via Vault PKI; cert +
      key stored via `SecretStore`.
- [ ] Cloud-init template embeds the cert + key; the agent
      binary fetches them at boot.
- [ ] Cert-rotation workflow + `GracefulRestart` command
      end-to-end test.
- [ ] `agent_audit` migration + per-cluster chain.
- [ ] Listener migration: production address + default-on
      flag.
- [ ] Each tunnel drop fires a structured `tracing` event
      the `agent-heartbeat-missing.md` runbook can rely on.
- [ ] `docs/runbooks/agent-heartbeat-missing.md` updated
      with the actual diagnostic commands (replaces the
      placeholder fields named in Sprint 4 ticket 03).
- [ ] **Security DoD (every row)**:
  - [ ] mTLS handshake actually rejects unknown CAs
        (negative test).
  - [ ] Per-cluster cert is bound to cluster id at
        issuance; a stolen cert from cluster A cannot
        impersonate cluster B even if presented with the
        right body bytes.
  - [ ] No control-plane-initiated outbound to the user
        cluster (verified by the no-`dial`-syscall test).
  - [ ] Cert rotation works without losing the agent's
        connected-state (the `GracefulRestart` flow).
  - [ ] `agent_audit` rows are append-only and chained;
        chain verification matches the existing audit-chain
        verifier pattern.

## What this ticket deliberately does **not** do

- **CRD-shaped command authorisation.** Phase 3+ ticket: a
  policy layer that says "this org's agent can run RPC X
  but not RPC Y." Until then, a hand-coded allowlist gates
  the RPC catalog.
- **Per-RPC tracing all the way down to the in-cluster call.**
  W3C trace context propagation across the tunnel is a
  Sprint 6+ follow-up; for the first ship, propagate
  `request_id` only.
- **Self-rolled OpenSSL fallback path.** ADR-0014 names this
  as the alternative if Vault PKI slips; with #04 landing
  Vault, we pick Vault PKI here. The OpenSSL path is dead
  code that doesn't get written.

## References

- [`docs/backlog/sprint-4/03-agent-reverse-tunnel.md`](../sprint-4/03-agent-reverse-tunnel.md)
  — Sprint 4 parent ticket; the deferred-to-Sprint-5+
  section is exactly what this ticket closes.
- [ADR-0014](../../adr/0014-agent-reverse-tunnel-wire-format.md)
  — wire format + cert binding ratified in Sprint 4 #3.
- [ADR-0007](../../adr/0007-secret-management.md) — Vault
  is the cert-store backend.
- [`crates/agent-proto/proto/agent.proto`](../../../crates/agent-proto/proto/agent.proto)
  — wire format the agent binary speaks.
- [`crates/api/src/agent.rs`](../../../crates/api/src/agent.rs)
  — control-plane handler this ticket wires the cert
  interceptor onto.
