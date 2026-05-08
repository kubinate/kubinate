# Replace cluster-status polling with SSE

**Labels**: `area/api`, `area/frontend`, `sprint-3`
**Epic**: Sprint 1 ticket 06 carry-over
**Size**: 5

## Context

Sprint 1 ticket 06 deliberately chose 2-second polling because
SSE/WebSocket was Sprint-3+ scope. We're in Sprint 3. Polling works
but every active cluster page costs us 30 round-trips/minute.
Replacing it with Server-Sent Events lets us push updates only when
state changes, freeing up runtime for the dogfood migration.

## Acceptance criteria

- **Given** a logged-in user viewing a cluster status page, **when**
  the page mounts, **then** it opens an `EventSource` against
  `GET /v1/clusters/:id/events` (text/event-stream).
- **Given** a workflow step transition (e.g. `installing_k3s_server →
  installing_k3s_agents`), **when** the runner writes the
  `provisioning_workflows.current_step` update, **then** every
  connected client for that cluster receives an `event: step` SSE
  message within 1 second.
- **Given** the cluster reaches a terminal state (`ready` /
  `failed` / `destroyed`), **then** the server sends a final
  `event: terminal` message and closes the stream cleanly.
- **Given** the connection drops, **then** the SPA reconnects with
  exponential backoff up to 60s.

## Implementation notes

- Use Axum's `Sse` response type. The runner already mirrors steps
  through a `ProgressSink` — extend with a Tokio broadcast channel
  per cluster id; the SSE handler subscribes.
- The dashboard's existing 2s polling stays as a fallback for the
  first request (initial state) and for cluster lists where SSE
  per-row would be overkill.
- Keep-alive comment lines every 15s to defeat intermediary
  buffering.

## DoD

- [x] Vitest covers the EventSource happy path + the reconnect path
      via a mocked `EventSource`. Three new specs in
      `frontend/src/routes/app/clusters/[id]/page.test.ts`:
      `opens an EventSource and refetches on step events`,
      `closes the EventSource after a terminal event without polling
      kicking in`, and `falls back to polling after repeated SSE
      failures`.
- [x] The 2s polling code stays as a fallback only — no double
      load on the API when both run during a transition. The page
      starts in SSE-only mode (`stopPolling()` runs on the first
      `connected` event), and only `startPolling()` once SSE is
      abandoned past `SSE_FALLBACK_AFTER_RETRIES`.
- [x] Runbook stub: `sse-connection-leak.md` (operationally
      important; long-lived connections are easy to leak). Linked
      from `docs/runbooks/README.md`.
- [x] Cluster status page tests still pass. `npx vitest run` shows
      12/12 across 4 files (existing addon + new SSE specs).
