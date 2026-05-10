# Sprint 16 — Audit Log Pagination + Session Activity Tracking

## Goal
Make the audit log usable at scale (keyset pagination instead of a hard
`LIMIT 100`) and make the member list `last_active_at` field meaningful by
bumping `sessions.last_used_at` on each authenticated request.

## Tickets

### 16-01 Audit log cursor pagination (backend)
- `GET /v1/audit-log` gains `?before=<uuid>` and `?limit=<n>` query params.
- UUID v7 primary keys are time-ordered; `id < $before` gives a stable
  descending cursor with no secondary sort needed.
- Default limit 50, max 200, clamped server-side. Response stays a flat
  `Vec<AuditLogEntry>` — callers detect "has more" by checking
  `entries.length === limit`.

### 16-02 Frontend — "Load more" button on audit log page
- `listAuditLog` in `audit-log.ts` gains optional `{ before?, limit? }` opts.
- `PAGE_SIZE = 50` constant exported alongside the function.
- Audit log page appends the next page to `entries` on "Load more" click;
  button is hidden once the last page is smaller than `PAGE_SIZE`.

### 16-03 Session activity bump
- `bump_activity(pool, id)` added to `crates/identity/src/session.rs`:
  `UPDATE sessions SET last_used_at = now() WHERE id = $1 AND last_used_at < now() - interval '5 minutes'`
- Called fire-and-forget via `tokio::spawn` from `Actor` cookie resolution
  in `actor.rs`, so it never adds latency to the request path.
- Makes `last_active_at` in the member list reflect actual recent usage
  rather than session creation time.
