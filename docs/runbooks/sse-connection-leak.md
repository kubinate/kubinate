# Runbook — SSE connection leak on `/v1/clusters/:id/events`

**Severity**: SEV-3 (degraded but contained) → SEV-2 if it crosses the
            connection-pool ceiling and starts shedding HTTP requests.
**Alert source(s)**:
- `KubinateApiOpenConnectionsHigh` (Prometheus) — `tcp_active_connections`
  > 80 % of the pool ceiling for > 10 minutes.
- File-descriptor exhaustion alert from the host — `process_open_fds`
  approaching `process_max_fds`.
**Owner**: Cluster Orchestrator team.
**Last reviewed**: 2026-05-04

## Summary

Sprint 3 ticket 10 introduced a Server-Sent Events stream at
`GET /v1/clusters/:id/events`. Each connected browser holds an
HTTP/2 stream open for as long as the cluster is non-terminal. A
leak — sockets that survive past the workflow's terminal event, or
clients that never call `close()` — eats into the API's connection
pool and ultimately starves regular request handlers.

This runbook covers two scenarios:

1. The server keeps streams open *after* the corresponding cluster
   reached `ready` / `failed` / `destroyed`.
2. Clients hold connections open longer than expected (multi-tab
   abuse, or a runaway integration that doesn't honour the
   `terminal` event).

## Symptoms

- Active connection count climbs in lockstep with cluster page
  views, but **does not drop** when those clusters reach a terminal
  state.
- `kubinate_sse_active_connections` (gauge, when added — see
  follow-ups below) does not return to baseline overnight.
- Lots of long-lived `/v1/clusters/.../events` lines in the access
  log with `duration > 30 minutes`.
- New API requests start queueing or timing out at 30 s
  (`tower_http::timeout`) under otherwise-light load.

## Severity rubric

- **SEV-3**: leak detected by metric, no user-visible HTTP failures
  yet. Pool headroom > 25 %.
- **SEV-2**: HTTP latency rising or pool headroom < 25 %. Failed
  customer requests starting to appear.
- **SEV-1**: API is dropping requests broadly. Combined with the
  agent-heartbeat-missing runbook this can cascade to the dogfood
  cluster losing control-plane access.

## Immediate actions (first 5 minutes)

1. Ack the page.
2. Pull the active-connection-count chart from
   `Grafana / Kubinate API / Connections` (see ticket 07's eventual
   dashboard; for now, `kubectl -n kubinate exec deploy/api -- curl
   -s localhost:9100/metrics | grep tcp_active_connections`).
3. Confirm the leak is SSE-specific: filter the access log for
   `/clusters/.*/events` requests with `duration_ms > 60_000`. If
   the suspect requests are *not* SSE, this runbook isn't the right
   one — go to `db-pool-exhaustion.md` instead.

## Diagnosis

Find clusters with the most stale-looking subscribers:

```bash
# From the control-plane host:
journalctl -u kubinate-api --since "1 hour ago" \
  | grep "GET /v1/clusters/.*/events" \
  | awk '{print $NF, $0}' \
  | sort -n -r | head
```

Confirm the workflow truly terminated for those cluster ids:

```sql
SELECT id, status, status_reason, updated_at
FROM clusters
WHERE id = ANY(ARRAY['<id-1>', '<id-2>'])
  AND deleted_at IS NULL;
```

If the cluster row says `ready` / `failed` / `destroyed` but the
SSE stream is still open, the runner failed to publish a terminal
event for that workflow run. Check
`crates/workflows/src/runner.rs::publish_terminal` call sites — every
match arm that flips the cluster status should also call this
helper.

## Mitigations

Ordered by preference.

1. **Restart the API process.** Kills every open stream; clients
   reconnect via the EventSource backoff and re-converge to live
   subscribers only. Lowest risk; ~5–10 s of `/healthz` blip.
   ```bash
   kubectl -n kubinate rollout restart deployment/api
   ```

2. **Patch missing terminal publish.** If the diagnosis confirms a
   missing `publish_terminal` call, ship the fix on a hotfix branch
   — the leak will not stop until each future workflow can close
   its own stream.

3. **Tighten the keep-alive interval (temporary).** The default is
   15 s (`KeepAlive::new().interval(15s)`). Dropping to 5 s makes
   intermediaries reap idle sockets more aggressively at the cost
   of a small bandwidth bump. Set
   `KUBINATE_SSE_KEEPALIVE_SECS=5` and bounce the API. **Revert
   after the underlying fix lands** — this is a pressure valve,
   not a fix.

## Recovery / rollback

- `tcp_active_connections` returns to baseline ± noise within 60 s
  of a restart.
- The same cluster ids are not over-represented in the next hour's
  access log.
- A repro test (synthetic browser opening 50 streams) drops back to
  zero after the simulated workflow's terminal event is published.

## Communications

- SEV-3: no external comms needed. Note in the engineering channel
  so other operators know the API was just restarted.
- SEV-2 / SEV-1: status page update once HTTP errors are
  customer-visible. Use the `incident-comms-template.md`
  language.

## Follow-ups

The first time this runbook is exercised, file all of:

- A Prometheus gauge `kubinate_sse_active_connections{cluster_id=}`
  exposed by the SSE handler (the hub already knows the receiver
  count from `broadcast::Sender::receiver_count`).
- A wallclock test in `crates/api/tests/sse_terminal.rs` that
  verifies the stream closes within 1 s of the terminal publish.
- A linter rule (or a code-reviewer agent prompt) that flags any
  match arm flipping `clusters.status` without a sibling
  `publish_terminal` call.

## Related

- ADR-0003: Cluster provisioning orchestration.
- `provisioning-workflow-stuck.md`: when the workflow is the thing
  not making progress, start there.
- `db-pool-exhaustion.md`: connection-pool symptoms that look
  similar but are upstream of the SSE handler.
- Frontend: `frontend/src/routes/app/clusters/[id]/+page.svelte`
  (EventSource lifecycle + reconnect/fallback).
