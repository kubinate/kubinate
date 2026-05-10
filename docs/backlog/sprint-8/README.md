# Sprint 8 plan

**Sprint length**: 2 weeks (Phase 3, weeks 9–10 of 14).
**Capacity target**: 20–25 points.

## Goal

> Close the last user-facing gap in the invite flow (no page to
> accept an invite), give the dashboard a real summary panel so
> operators can see fleet health at a glance, and surface agent
> connection status on the cluster list cards now that
> `agent_last_seen_at` is available from Sprint 7.

## Scope

| # | Ticket | Pts | What ships |
|---|--------|-----|-----------|
| 01 | Invite acceptance page | 5 | `/accept?token=kinv_xxx` SvelteKit route (outside the `/app` guard). Calls `POST /v1/invites/accept`, redirects to `/app` on success. Team page emits a full URL instead of the raw token so the link is copy-paste ready. |
| 02 | Dashboard summary panel | 5 | Aggregate stats row above the cluster list: total clusters, workers, agents online (within 90 s of `agent_last_seen_at`), status badge breakdown. Pure frontend — no new API. |
| 03 | Agent status on cluster list cards | 3 | Show a connected/disconnected badge on each cluster card in the dashboard list, matching the badge already on the cluster detail page. |
| 04 | `acceptInvite` frontend API fn | 2 | Add `acceptInvite(token)` to `frontend/src/lib/api/team.ts` and the matching `acceptInviteResponseSchema` to `schemas.ts`. Needed by ticket 01. |

**Total: 15 points.** Deliberately light — Sprint 8 is the
"polish and close the UX seams" sprint between the infrastructure
heavy Sprint 5 and any Phase 3 exit-gate work.

## Dependency graph

```
#04 acceptInvite fn ──► #01 invite page
#02 dashboard stats (independent)
#03 agent badge on list (independent)
```

## What "done" looks like at sprint close

- [ ] `cargo test --workspace` green.
- [ ] `npm run check && npm run lint && npm test -- --run` green.
- [ ] Navigating to `/accept?token=kinv_abc` when logged in joins
      the org and redirects to `/app`.
- [ ] Navigating to `/accept?token=kinv_abc` when not logged in
      preserves the token through the login redirect.
- [ ] Dashboard shows total clusters / workers / agents-online /
      status breakdown above the cluster grid.
- [ ] Cluster list cards show a green/red agent badge for
      `ready` + `scaling` clusters.
- [ ] Team page emits the full `[origin]/accept?token=...` URL
      in the issued-token banner.
