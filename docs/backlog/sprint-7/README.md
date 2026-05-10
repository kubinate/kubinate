# Sprint 7 — Agent liveness + cluster lifecycle UI

**Theme**: Close the agent feedback loop (heartbeats persist to DB; UI shows "connected/disconnected") and complete the cluster lifecycle in the frontend (destroy with confirmation).

## Tickets

| # | Title | Status |
|---|-------|--------|
| 01 | Persist agent heartbeat to `clusters` table | this sprint |
| 02 | Cluster destroy UI — button + confirm dialog | this sprint |
| 03 | Agent `MetricsRemoteWrite` org-id cross-check | this sprint |

## Ticket 01 — Agent heartbeat persistence

**Migration**: Add two columns to `clusters`:
- `agent_last_seen_at TIMESTAMPTZ` (nullable — `NULL` = never connected)
- `agent_version TEXT NOT NULL DEFAULT ''`

**Repository**: `ClusterRepository::touch_agent_heartbeat(org_id, cluster_id, agent_version)` — `UPDATE clusters SET agent_last_seen_at = now(), agent_version = $3 WHERE id = $1 AND organization_id = $2`.

**API wire**: Thread `PgPool` into `ApiAgentService` so the heartbeat arm can call `touch_agent_heartbeat`. The `cluster_id` field on each `Heartbeat` message is a string UUID — parse and pass through.

**ClusterView + schemas**: Expose `agent_last_seen_at: Option<OffsetDateTime>` and `agent_version: String` in the API response.

**Frontend**: On the cluster detail page, show an "Agent connected" badge (green) when `agent_last_seen_at` is within the last 90 seconds, or "Agent disconnected" (grey) otherwise. Show the last-seen timestamp as "X ago" text.

## Ticket 02 — Cluster destroy UI

`DELETE /v1/clusters/:id` already exists (OwnerActor-gated, returns 202). The frontend has a placeholder `retry()` stub but no destroy button.

Add:
- `destroyCluster(id)` to `frontend/src/lib/api/clusters.ts`
- A "Destroy cluster" button in the cluster detail page (only when status is `ready`, `failed`, or `scaling` — not when already `destroying` or `destroyed`)
- Confirmation dialog: "Type the cluster name to confirm"
- On confirm → call `destroyCluster` → navigate to `/app/clusters` (list view)
- Disable the button when in-flight

## Ticket 03 — Agent org-id cross-check

When processing `AgentPayload::Metrics(m)`, the `m.organization_id` field is agent-supplied (untrusted in the security model). Thread the `PgPool` into `ApiAgentService` (already needed for ticket 01); after the first heartbeat populates `cluster_id`, look up the cluster's `organization_id` from the DB and compare against `m.organization_id`. Log a warning and drop the payload if they differ.

**Why now**: Sprint 7 already threads the pool for ticket 01; the cross-check is a one-liner on top. Failing to validate means a compromised agent binary could forge any tenant's `organization_id` and inject metrics into another tenant's namespace, bypassing the label injection in VM.

## Definition of done

- `cargo test --workspace` green.
- `npm run check && npm run lint` green.
- A heartbeat over the loopback test updates `agent_last_seen_at` in the test DB (or the unit test covers the repository method directly).
- "Destroy cluster" button visible and functional on the cluster detail page for ready clusters.
- `AgentPayload::Metrics` org-id mismatch logs a warning and skips `forward_write`.
