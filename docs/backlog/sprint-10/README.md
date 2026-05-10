# Sprint 10 plan

**Sprint length**: 2 weeks (Phase 3, weeks 13–14 of 14).
**Capacity target**: 20–25 points.

## Goal

> Surface the audit log that has been accumulating in `audit_log_entries`
> since Sprint 1. Owners need a read-only view of who did what and when
> — passkey registrations, kubeconfig downloads, invite acceptances,
> billing changes, and more — without having to query the database
> directly.

## Scope

| # | Ticket | Pts | What ships |
|---|--------|-----|-----------|
| 01 | `GET /v1/organizations/:id/audit-log` | 5 | New endpoint in `crates/api/src/audit_log.rs`. Returns the 100 most recent `audit_log_entries` rows for the tenant, joining `users` for `actor_email` and `actor_display_name`. Gated behind `OwnerActor` (admins can read, developers/viewers cannot). |
| 02 | Audit log frontend schema + API fn | 3 | `auditLogEntrySchema` in `schemas.ts`; `listAuditLog(orgId)` in a new `frontend/src/lib/api/audit-log.ts`. |
| 03 | `/app/settings/audit-log` page | 5 | New settings page. Loads the last 100 entries via `onMount`. Renders a table: timestamp · actor · action · resource · decision. Pretty-prints the `action` string (underscores → spaces, dot separator). |
| 04 | Audit log nav link | 2 | Adds "Audit log" (Shield or FileText icon) to the Settings group in the app layout sidebar. |

**Total: 15 points.**

## What "done" looks like at sprint close

- [ ] `GET /v1/organizations/:id/audit-log` returns 200 with a JSON
      array for Owner and Admin actors; returns 403 for Developer/Viewer.
- [ ] Page at `/app/settings/audit-log` renders the table with real
      data after `onMount`.
- [ ] Sidebar nav includes "Audit log" entry that highlights when active.
- [ ] `cargo test --workspace` and `npm test -- --run` green.
- [ ] `cargo clippy` and `npm run lint` clean.
