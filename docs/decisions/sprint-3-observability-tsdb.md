# Sprint 3 spike — Multi-tenant TSDB + log store

**Status**: Proposed (recommendation **VictoriaMetrics for TSDB,
            S3-backed log store, defer Loki**, conditional on the
            measurements below).
**Decided**: <<2026-05-04; ratify after the spike runs>>
**Spike owner**: Observability team (Cluster Orchestrator team
            owns the proxy code; Observability team owns the
            backend-store choice).
**Relates to**: ADR-0004 (deployment topology), ADR-0006
(multi-tenancy isolation), ADR-0011 (defer Temporal —
metrics infrastructure that this spike builds on).

---

## ⚠️ Honest scope note

This file is a **decision-doc starter**, not the spike itself. The
empirical sections below are marked `<<measurement needed>>` — the
spike-runner fills those in by actually building / running. Do not
ratify the recommendation until those markers are resolved.

The structure here is the value: it picks the questions the spike
must answer, lists the tradeoffs we can already assert from prior
art, and pre-commits the team to a shape for the recommendation so
the spike doesn't drift into open-ended exploration.

---

## Context

The brief lists "managed observability without per-cluster Prometheus
sprawl" as a Phase 2 finisher. ADR-0004's container diagram already
draws an Observability Proxy as one of the four control-plane services.
The `crates/observability/` crate exists today as a placeholder; this
spike fills it.

The forcing function is Phase 3: dogfood-cluster migration is on
the marketing roadmap (week 22+), and we cannot run our own k3s
without scraping it from somewhere. Phase 2 ships the smallest
end-to-end metric round-trip we can stand behind in production for
the first paying alpha tenants.

The decisions to make:

1. **Multi-tenant TSDB**: Mimir vs. VictoriaMetrics for cluster-
   resident metrics that arrive via the agent's reverse tunnel.
2. **Log store**: Loki vs. an S3-backed shape (Parquet + a small
   index) for Phase 2 — full Loki adoption is on the table for
   Phase 3 if the S3 shape proves too painful.
3. **Multi-tenant query path**: how a tenant's dashboard fetch
   becomes a query scoped to their `organization_id` without
   cross-tenant leakage. This is the security-critical invariant —
   the tenancy story must hold even when the TSDB itself does not
   speak per-tenant authz.

## Spike scope

Three days of focused work, time-boxed. Deliverable:

1. The smallest possible scaffold under `crates/observability/`:
   a Tokio-resident ingest endpoint, an in-memory store keyed by
   `organization_id`, and a tenant-scoped query function. Wire
   format: JSON for the skeleton (the production target is
   Prometheus remote-write protobuf, which adds protobuf + snappy
   to the workspace and waits on the real agent reverse tunnel).
2. A round-trip smoke test: ingest as tenant A, query as A returns
   the points, query as B returns nothing.
3. This decision doc filled in.

Do **not** spike: production deploy of either Mimir or
VictoriaMetrics, Loki cluster topology, Grafana embed (Sprint 4+).

## What we can assert from prior art

These items don't need empirical measurement — they're stable
facts about the ecosystem we're entering.

### Mimir vs. VictoriaMetrics

Both are well-trodden multi-tenant Prometheus-compatible TSDBs. The
trade-offs at our Phase 2 scale (tens of clusters, hundreds of
series per cluster) are:

| Aspect | Mimir | VictoriaMetrics |
|---|---|---|
| Operational complexity | High — microservices (ingester, querier, ruler, store-gateway, compactor). | Lower — fewer binaries (`vmstorage` + `vmselect` + `vminsert` cluster mode, or single-node `vm` for small fleets). |
| Multi-tenancy | First-class via the `X-Scope-OrgID` header. | First-class via path-prefixed tenant id (`/insert/<tenant>/...`, `/select/<tenant>/...`). |
| Query language | PromQL. | MetricsQL (PromQL superset; documented divergences are small). |
| Hardware footprint | Heavier — designed for Cortex-class scale. | Lighter — single-node handles tens of millions of active series. |
| Ecosystem | CNCF graduated; matches Cortex history. | CNCF Sandbox; smaller community but very active. |
| License | AGPL-3.0. | Apache-2.0. |

We do not need Cortex-class scale in Phase 2 or even Phase 3 alpha.
The Apache-2.0 license, lower op cost, and single-binary deploy
favour VictoriaMetrics. The risk we accept is the smaller ecosystem
— if a Phase 4+ scale-up forces a migration, the wire format
(remote-write) means clients don't change.

### Loki vs. an S3-backed log store

Loki is the Grafana-stack default and integrates with the agent's
existing log-shipping conventions, but:

- Loki's storage layout assumes multi-tenant (`X-Scope-OrgID` again),
  which is convenient for us, but the operational story
  (object-store + ingester + querier + compactor) doubles the
  Phase 2 ops surface.
- For the Phase 2 alpha (≤ 10 paying clusters), an S3-backed shape
  using Parquet files + a small Postgres index gets us: durability,
  per-tenant prefixes, and queries that fit our scale. The
  spike-runner builds the prototype to confirm the index-design
  assumption.
- Phase 3+ revisits Loki when the dogfood cluster's own logs need
  to land here.

### Multi-tenant query path

Whichever TSDB and log store we land on, **the tenancy boundary is
enforced at the proxy**, not the backend. Concretely:

- Every ingest request carries an authenticated tenant id; the
  proxy never trusts a client-supplied label like
  `__tenant_id__`.
- Every query request reads the tenant id from the authenticated
  session (`Actor::organization_id`) and passes it as the
  vendor-specific tenant header to the backend.
- The backend's own multi-tenant feature is treated as defence in
  depth, not the primary control. If the backend ever leaks across
  tenants in a major-version upgrade, the proxy still doesn't.

This is the same pattern as the API edge's `TenantScopedTransaction`
(ADR-0006): isolation is a property of the proxy, audited by tests
that try to leak.

### Prometheus remote-write as the wire

Both candidate TSDBs accept the standard remote-write protobuf
shape, and the Prometheus agent / VictoriaMetrics' `vmagent` /
Grafana Agent all emit it. Picking remote-write means:

- No bespoke ingest format.
- The agent crate (when its mTLS reverse tunnel ships) wraps a
  vmagent or its own remote-write client without touching the
  proxy's wire shape.
- The skeleton this spike ships uses **JSON**, not remote-write,
  because the protobuf + snappy machinery is meaningless until the
  reverse tunnel exists. The schema is shaped to round-trip 1:1
  with remote-write so the swap is mechanical.

## What needs measurement (the actual spike)

### M1 — VictoriaMetrics single-node footprint

Stand up `victoria-metrics` (single-node) on the existing Phase 2
VPS. Push 1k samples/s synthetic traffic for 24h.

- **Idle RSS**: <<measurement needed>>
- **Steady-state RSS**: <<measurement needed>>
- **Disk usage / day**: <<measurement needed>>
- **p99 query latency for a 1h range over a 50-series tenant**: <<measurement needed>>

These set the budget for "VPS keeps running everything" vs.
"observability moves to its own host" in Phase 3.

### M2 — Cross-tenant isolation

Submit two synthetic ingest streams from tenant A and tenant B.
Issue queries as A and as B. Confirm:

- A's queries see only A's series.
- B's queries see only B's series.
- A query from a third (unauthenticated or wrong-tenant) actor
  returns 401/403, never the data.

The proxy's test does this in-process for the JSON skeleton; the
backend test repeats it against a live VictoriaMetrics.

- **Result**: <<measurement needed>>

### M3 — S3 log scaffold

Smallest workable shape: per-tenant prefix in S3, Parquet files
rotated hourly, a Postgres `log_index` table mapping (org_id,
hour, file) → row count. Smoke test: write 10k log lines for
tenant A, query a 30-min window, expect every line; query as
tenant B, expect zero.

- **Wallclock for 10k-line query**: <<measurement needed>>
- **Bytes / 10k-line tenant-hour**: <<measurement needed>>

### M4 — Operational footprint vs. Loki

If M3's wallclock is acceptable, the comparison is purely
operational:

- Loki single-node or microservices? — <<measurement needed>>
- S3 + Parquet adds a Parquet writer dep (`parquet` crate) — <<measurement needed>>

## Recommendation skeleton

The recommendation flips on M1, M2, and M3:

| M1 footprint | M2 isolation | M3 S3 scaffold | Recommendation |
|---|---|---|---|
| Acceptable | Holds | Hits wallclock target | **VictoriaMetrics + S3** for Phase 2; revisit Loki in Phase 3. |
| Acceptable | Holds | S3 fails | **VictoriaMetrics + Loki** in Phase 2 — eat the op cost. |
| Acceptable | Leaks | n/a | **Stop** — proxy boundary needs redesign before any backend choice is meaningful. |
| Footprint exceeds VPS budget | n/a | n/a | **Move observability off the VPS** to a dedicated host before Phase 3 dogfood. |

Default recommendation, until M1–M4 are filled in: **VictoriaMetrics
+ S3-backed log store**. Single-binary VM deploy fits the
single-VPS topology; the S3 log shape avoids paying for Loki ops
twice (once now, once on dogfood k3s).

## What "VictoriaMetrics + S3" looks like in practice

Sprint 4+ tickets sized after M1–M3 are in:

- Deploy single-node `victoria-metrics` via Ansible (extend
  `infra/ansible/`).
- Wire the agent's reverse tunnel (separate Sprint 4 ticket) to
  push remote-write through the proxy, which forwards to VM with
  the tenant header.
- Write the S3 log scaffold (Parquet writer + Postgres index +
  range query).
- Stand up Grafana with a per-org datasource that goes through
  the proxy (datasource-level tenant isolation; users never get
  raw VM URLs).

## What this ticket ships

The placeholder `crates/observability/` becomes:

- `metrics::ingest` — in-process JSON ingest. Tenant-scoped.
- `metrics::store` — `Arc<MetricsStore>` with an in-memory
  per-tenant `Vec<Sample>`. Behind a trait so the live VM-backed
  impl can land later without rewriting the API layer.
- `metrics::query` — `query_range(org_id, matchers, time range)`.
- API endpoints under `/v1/observability/`:
  - `POST .../write` (per-tenant agent auth — for now, asserts
    `Actor::organization_id` matches the body's claimed tenant).
  - `GET .../query` (uses `Actor::organization_id` as the only
    valid scope).

This is **not** the production scaffold. It is the smallest
runnable thing that exercises the proxy's tenant-isolation
invariant, so the M2 measurement above is meaningful when we
stand up the real VM.

## Revisit trigger

This decision re-opens automatically when **any** of the
following observables fire:

1. M1's steady-state RSS exceeds the VPS budget. The
   in-flight workflow gauge from ADR-0011 is the canary; if the
   VPS is at-risk for any reason we re-spike observability
   topology before adding load.
2. The proxy's tenant-isolation test ever fails. Fix is
   immediate; the post-mortem may surface a structural issue
   that demands a different backend.
3. Phase 3 begins. The dogfood-cluster milestone implies
   running observability on top of our own k3s, which changes
   the deployment story for either choice.

## References

- ADR-0004: Deployment topology.
- ADR-0006: Multi-tenancy isolation model.
- ADR-0011: Defer the Temporal SDK adoption (the metrics
  infrastructure this spike's gauge lives on).
- VictoriaMetrics docs: `https://docs.victoriametrics.com/`.
- Mimir docs: `https://grafana.com/docs/mimir/`.
- Loki docs: `https://grafana.com/docs/loki/`.
- Prometheus remote-write spec:
  `https://prometheus.io/docs/specs/remote_write_spec/`.
- `crates/observability/`: the scaffold this spike fills.
- `docs/backlog/sprint-3/07-observability-proxy.md`: the ticket
  this doc closes.
