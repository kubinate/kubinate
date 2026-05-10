# Sprint 9 plan

**Sprint length**: 2 weeks (Phase 3, weeks 11–12 of 14).
**Capacity target**: 20–25 points.

## Goal

> Complete the cluster lifecycle surface: expose the worker-scaling
> endpoint that has been on the backend since Sprint 3 but had no UI,
> surface the authenticated user's email/display_name through `/v1/me`
> (the sidebar currently shows a truncated UUID), and polish page
> titles so browser tabs are navigable.

## Scope

| # | Ticket | Pts | What ships |
|---|--------|-----|-----------|
| 01 | Worker scaling UI | 8 | `+` / `−` controls on the cluster detail page for `ready` clusters call `POST /v1/clusters/:id/workers` with `{delta: ±1}`. The API enforces OwnerActor; a failed 403 shows an inline error. After success `refresh()` picks up the new `scaling` status via SSE/poll. |
| 02 | `/v1/me` adds email + display_name | 5 | Backend: extend `MeView` with `email` and `display_name` queried from the `users` table. Frontend: update `meViewSchema`; sidebar shows `display_name` (falling back to email) instead of a truncated UUID. |
| 03 | Dynamic page titles | 2 | `<title>{cluster.name} — Kubinate</title>` on the cluster detail page (reactive once the first fetch completes). All other pages already have static titles. |
| 04 | `scaleWorkers` frontend API fn | 2 | Add `scaleWorkers(clusterId, delta)` to `clusters.ts`. Returns void (202 consumed; 200 no-op ignored). Needed by ticket 01. |

**Total: 17 points.**

## What "done" looks like at sprint close

- [ ] Clicking `+` on a 2-worker ready cluster fires `POST …/workers`
      `{delta:1}`, cluster enters `scaling`, worker count updates to 3.
- [ ] `−` is disabled when `worker_count === 1`; `+` is disabled at 10.
- [ ] `GET /v1/me` response includes `email` and `display_name`.
- [ ] Sidebar footer shows display_name / email instead of UUID.
- [ ] Browser tab for `/app/clusters/abc-123` reads "abc-123 — Kubinate".
- [ ] `cargo test --workspace` and `npm test -- --run` green.
