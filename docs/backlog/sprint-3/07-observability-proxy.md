# Multi-tenant metric + log query proxy (skeleton)

**Labels**: `area/observability`, `sprint-3`
**Epic**: Phase 2 finisher
**Size**: 8 (spike-shaped)

## Context

The `observability` crate exists as a placeholder. Per ADR-0004's
container diagram, the Observability Proxy is one of the four
control-plane services (alongside the API, the orchestrator, and the
add-on manager). Phase 3 dogfood requires we have *something*
running — even minimal — before we move onto our own k3s.

This ticket is **spike-shaped**: the deliverable is a decision doc +
the smallest scaffold that runs end-to-end against the existing
agent reverse tunnel.

## Acceptance criteria

- **Given** the spike, **when** it concludes, **then**
  `docs/decisions/sprint-3-observability-tsdb.md` covers:
  - **Mimir vs. VictoriaMetrics** for the multi-tenant TSDB
    (cost, operational complexity, query API).
  - **Loki** vs. a simpler S3-backed log store for the Phase 2
    scope.
  - Multi-tenant query path: how a tenant's dashboard fetch
    becomes a query scoped to their `organization_id` without
    cross-tenant leakage.
- **Given** the recommendation, **then** the `observability` crate
  has an end-to-end smoke test: agent pushes a synthetic metric
  via the reverse tunnel, the proxy stores it, and an authenticated
  dashboard query for the right tenant retrieves it; a query for the
  wrong tenant returns nothing.

## Implementation notes

- Reuse the agent's existing mTLS reverse tunnel (Sprint 1 ticket
  03 + agent crate) to ship metrics back. The agent already
  authenticates per-cluster.
- Prometheus remote-write protocol is the lowest-friction first cut.
- Defer Grafana embed to Sprint 4; the smoke test can use raw HTTP.

## DoD

- [x] Decision doc merged. See
      [`docs/decisions/sprint-3-observability-tsdb.md`](../../decisions/sprint-3-observability-tsdb.md):
      Mimir vs VictoriaMetrics, Loki vs S3-backed log store,
      multi-tenant query path. Default rec **VictoriaMetrics +
      S3-backed log store** (single-binary VM fits the single-VPS
      topology; S3 avoids paying for Loki ops twice). Empirical
      sections honestly marked `<<measurement needed>>`.
- [x] Smoke test against a single-tenant metric round-trip.
      `round_trip_returns_only_the_writers_samples` in
      `crates/observability/src/metrics.rs::tests`.
- [x] Cross-tenant query returns empty (no rows leak).
      Same test asserts tenant B sees nothing of tenant A's data;
      `tenant_mismatch_in_write_body_is_rejected` covers the
      defence-in-depth case where a misbehaving agent claims a
      different `organization_id` in the body.
- [x] Runbook stub:
      [`docs/runbooks/observability-proxy-down.md`](../../runbooks/observability-proxy-down.md).
      Indexed in `docs/runbooks/README.md`.

## What ships under the hood

- `crates/observability/src/metrics.rs` — `MetricsStore` trait +
  `InMemoryMetricsStore` scaffold + tenant-isolation guards on
  ingest.
- `crates/api/src/observability.rs` — `POST /v1/observability/write`
  and `POST /v1/observability/query`, both tenant-scoped via the
  authenticated `Actor`.
- `AppState` carries the store as `Arc<dyn MetricsStore>`; swapping
  to a VictoriaMetrics-backed impl in Phase 4+ is a single-binding
  change at startup.
