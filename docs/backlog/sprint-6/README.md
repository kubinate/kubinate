# Sprint 6 — Cluster metrics pipeline

**Theme**: Wire the agent's `MetricsRemoteWrite` payload end-to-end into VictoriaMetrics and expose a cluster-scoped metrics query API + frontend chart.

## Tickets

| # | Title | Status |
|---|-------|--------|
| 01 | Wire agent `MetricsRemoteWrite` → `MetricsStore::forward_write` | this sprint |
| 02 | `GET /v1/clusters/:id/metrics` range query endpoint | this sprint |
| 03 | Cluster detail metrics chart (SVG sparkline, no dep) | this sprint |
| 04 | CloudNativePG migration stub (infra/Ansible — separate PR) | deferred |

## Ticket 01 — Agent metrics forwarding

**Goal**: The `AgentPayload::Metrics` arm in `agent.rs::open_stream` currently logs-and-drops. Wire it to forward the raw Prometheus remote-write bytes to the configured `MetricsStore`.

**Approach**:

1. Add `forward_write(organization_id: Uuid, write_request: Vec<u8>) -> Result<(), MetricsError>` to the `MetricsStore` trait in `crates/observability/src/metrics.rs`.
   - `InMemoryMetricsStore`: no-op (protobuf bytes can't feed the in-memory scalar store; return `Ok(())`).
   - `VictoriaMetricsStore`: POST to `{base_url}/api/v1/write?extra_label=organization_id={org}` with `Content-Type: application/x-protobuf`, `Content-Encoding: snappy`, `X-Prometheus-Remote-Write-Version: 0.1.0`. VM injects the label server-side, giving us the same tenant-isolation guarantee as the text-import path.

2. Add `metrics_store: Arc<dyn MetricsStore>` to `ApiAgentService`. Thread it from `main.rs::spawn_if_enabled(state.metrics_store.clone())`.

3. In the `AgentPayload::Metrics(m)` arm: parse `m.organization_id` as a UUID, call `metrics_store.forward_write(org_id, m.write_request).await`, log on error.

**Tenant isolation note**: the `organization_id` in the protobuf field is agent-supplied (untrusted). Sprint 7+ validates it against the cluster's org id looked up by mTLS cert serial. For now, we trust the field (the agent is the only writer and mTLS is the auth gate) and inject it as an extra label so cross-tenant queries are structurally blocked at VM.

## Ticket 02 — Cluster metrics query API

`GET /v1/clusters/:id/metrics?metric=<name>&start=<unix_s>&end=<unix_s>`

- Verify cluster belongs to `actor.organization_id`.
- Build a `RangeQuery` and call `state.metrics_store.query_range(actor.organization_id, &q)`.
- Return `{ samples: Vec<Sample> }`.
- Cluster scope: the endpoint is under `/{id}/metrics` to make auth obvious; the RLS is the org-level `organization_id` label VM injects at ingest.

## Ticket 03 — Cluster detail chart

SVG sparkline inline on `routes/app/clusters/[id]/+page.svelte`. No external chart lib — the data shape is simple enough (timestamp_ms + value pairs). Shows last 30 min of `kubinate_agent_heartbeat_total` (one series, one line).

## Definition of done

- `cargo test --workspace` green.
- `npm run check && npm run lint` green.
- `AgentPayload::Metrics` arm no longer logs-and-drops.
- `GET /v1/clusters/:id/metrics?metric=kubinate_agent_heartbeat_total&start=...&end=...` returns valid JSON from a VM-backed deploy.
- Chart renders (or gracefully shows "no data") on the cluster detail page.
