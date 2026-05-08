# Production observability proxy on VictoriaMetrics + S3 logs

**Labels**: `area/observability`, `area/platform`, `sprint-4`
**Epic**: ADR-0011 + Sprint 3 ticket 07 follow-on
**Size**: 13 (size depends on which spike-recommendation row in
[`docs/decisions/sprint-3-observability-tsdb.md`](../../decisions/sprint-3-observability-tsdb.md)
the M1–M4 measurements pick; the table below is the
**default-rec** path)

## Context

Sprint 3 ticket 07 shipped the observability proxy as a thin
in-process scaffold in `crates/observability/`:
`InMemoryMetricsStore` behind a `MetricsStore` trait + JSON
ingest + tenant-isolation tests. The decision-doc starter at
[`docs/decisions/sprint-3-observability-tsdb.md`](../../decisions/sprint-3-observability-tsdb.md)
ratifies the **default rec** of **VictoriaMetrics + S3-backed
log store**. This ticket runs the M1–M4 measurements against the
dogfooded cluster, ratifies the recommendation in a new ADR, and
ships the production scaffold.

The roadmap names "Production observability proxy in front of a
real VictoriaMetrics" as a Phase 3 exit gate
(`docs/roadmap.md`).

## When this opens

After Sprint 4 tickets **02 (Vault)** and **03 (agent reverse
tunnel)** land. The proxy is the agent tunnel's first real
consumer; Vault issues the agent's per-cluster credential. Trying
to ship this ticket before either of those leaves the
authentication story half-baked (back to env-var-mounted creds)
and the wire transport unsolved (back to direct HTTPS, defeating
the inbound-only invariant from CLAUDE.md).

## Sprint 4 status: **deferred to Sprint 5+**

This ticket is hard-blocked on M1–M4 measurements that need a
**running VictoriaMetrics on the dogfood cluster**, which the
Sprint 4 plan does not yet deliver
(see `docs/decisions/sprint-4-dogfood-migration.md`). Pre-coding
the `VictoriaMetricsStore` before knowing the shape from the M1
footprint measurement would be invention — the implementation
shape changes materially between "single-node VM fits the host"
and "must run a cluster-mode VM."

Sprint 4 does **no** work on this ticket. The in-process
`InMemoryMetricsStore` from Sprint 3 ticket 07 stays the
production observability backend until Sprint 5 / 6 delivers
the dogfood cluster and the spike runs. The decision-doc
starter at
`docs/decisions/sprint-3-observability-tsdb.md` remains the
authoritative shape for what this ticket eventually ships.

If Sprint 5 / 6 fail to deliver the dogfood cluster, this
ticket re-opens for a different plan: a single-VPS-resident
VictoriaMetrics (likely as a sidecar to the existing
`kubinate-api` process). That fallback is **not** the current
plan and should not be sized speculatively.

## Acceptance criteria — Default-rec path (VictoriaMetrics + S3)

- **Given** a VictoriaMetrics single-node deploy in the
  dogfooded cluster, **when** the agent (Sprint 4 ticket 03) makes
  a `metrics_remote_write` RPC over the reverse tunnel, **then**
  the control-plane proxy translates it into a Prometheus
  remote-write protobuf POST and forwards to VM with
  the per-tenant prefix (`/insert/<organization_id>/...`).
- **Given** an authenticated user query for the right tenant,
  **when** the dashboard calls `POST /v1/observability/query`,
  **then** the proxy translates it into a VictoriaMetrics
  PromQL query against `/select/<organization_id>/...` and
  returns the samples.
- **Given** a query for a different tenant, **when** the actor's
  session tenant doesn't match the requested data,
  **then** the proxy returns empty before the VM call (the proxy
  is the trust boundary; VM's path scoping is defence in depth).
- **Given** a 10k-line log stream from one tenant, **when** stored
  in S3 under `logs/<organization_id>/<hour>.parquet` and indexed
  in `log_index` Postgres, **then** a 30-min range query returns
  every line in < 5s p99.

## Acceptance criteria — Mimir or Loki branches

If M1 (footprint) or M3 (S3 wallclock) measurements force a
different recommendation, the AC + DoD shape changes:

- Mimir: same multi-tenancy contract, different vendor header
  (`X-Scope-OrgID`), different ops surface (microservices).
  Re-validate footprint on the dogfooded cluster's resource
  budget.
- Loki: replaces S3-Parquet for logs. Adds a Loki single-node
  deploy + `vmagent`-style log shipping in the agent binary.

## Spike measurements (block this ticket on these)

Per the decision-doc starter, fill in:

- **M1** — VictoriaMetrics single-node footprint on the
  dogfooded cluster (idle RSS / steady-state RSS / disk-per-day /
  p99 query latency).
- **M2** — Tenant-isolation round-trip against a live VM
  (already covered for the in-process store; repeat against
  the live deploy).
- **M3** — S3 + Parquet log scaffold: 10k-line write, 30-min
  query wallclock, bytes per tenant-hour.
- **M4** — Operational comparison: VM single-node vs VM cluster
  vs Mimir microservices for our scale.

The ratifying ADR (0014 or next free) cites the M1–M4 results
and names which row from the recommendation matrix fired.

## Implementation notes

- The `MetricsStore` trait already gates the swap. New impl
  `VictoriaMetricsStore` lives at
  `crates/observability/src/metrics/victoria.rs`; the
  `InMemoryMetricsStore` stays compiled (used by tests +
  optional `KUBINATE__OBSERVABILITY_BACKEND=memory` for local
  dev).
- VM client: use `reqwest` (already a workspace dep) to call
  remote-write directly. No need for a vendored vmclient crate.
- Logs path: new `crates/observability/src/logs/` module with
  the same trait shape (`LogStore`) and an `S3ParquetLogStore`
  impl. The placeholder `crates/observability/src/logs.rs` is
  the parking lot.
- Wire format: the agent already speaks remote-write to the
  proxy (per ticket 03's design); the proxy speaks remote-write
  to VM. **Don't transcode through JSON in the middle** — the
  protobuf passes through so the proxy is just a tenant-routing
  + auth layer.
- Multi-tenant query path: see `docs/decisions/sprint-3-observability-tsdb.md`'s
  "Multi-tenant query path" section. The trust boundary is the
  proxy; VM's path-prefixed multi-tenancy is defence in depth.

## DoD

- [ ] `VaultStore`-backed agent credentials (depends on ticket 02)
      authenticate the agent's `metrics_remote_write` call.
- [ ] `VictoriaMetricsStore` impl with parity tests against the
      `InMemoryMetricsStore` (every public test in
      `crates/observability/src/metrics.rs::tests` passes against
      both backends via the trait).
- [ ] `S3ParquetLogStore` impl + `log_index` Postgres table +
      migration. Tenant-isolation tests repeated for logs.
- [ ] M1–M4 measurements filled into the spike-doc starter
      (`docs/decisions/sprint-3-observability-tsdb.md`); the doc
      transitions from "Proposed" to a ratified status on the
      decision-doc footer.
- [ ] ADR-0014 (or next free) ratifies the chosen backends and
      cites the M1–M4 numbers. **Supersedes** the relevant
      portion of the spike-doc starter, leaving it as the
      historical record.
- [ ] `docs/runbooks/observability-proxy-down.md` updated with
      the live diagnostic commands (today it has placeholder
      `kubinate_observability_*` Prometheus gauges; this ticket
      provides them).
- [ ] Threat-model v2 captures the new boundary: API ↔ VM ↔ S3.
      The current v1 footer names this as a v2 trigger.
- [ ] No regression on the in-process backend: existing
      `crates/observability/src/metrics.rs::tests` continue to
      pass.

## What this ticket deliberately does **not** do

- **Per-cluster Prometheus scraping config.** The agent
  buffers + ships; we don't reuse Prometheus's federation. That's
  Phase 4 if customers ask.
- **Grafana embed.** The dashboard talks to the proxy directly
  in Phase 3; Grafana datasource integration is Phase 4
  enterprise-readiness scope.
- **Trace storage.** Tempo / Jaeger backend is its own decision
  and a different forcing function.
- **Auto-scaling VM cluster.** Single-node VM is the Phase 3
  target. If M1 says single-node won't fit, escalate to a
  cluster-mode follow-up; don't try to ship both in the same
  sprint.
