# Sprint 14 — Organization Settings Page

## Goal
Add a "General" settings page where owners can view and update the
organization display name. Closes the last gap in the settings sidebar nav.

## Tickets

### 14-01 `GET /v1/organizations/:id` + `PATCH /v1/organizations/:id`
- `GET` — any member (Actor); returns `{ id, slug, display_name, created_at }`.
- `PATCH` — Owner only (OwnerActor); accepts `{ display_name }`, validates
  1–100 chars after trim, returns the updated view.
- Both routes added to `team::org_routes()` (already nested at
  `/v1/organizations/{org_id}`).
- Tenant scope enforced via `SET LOCAL` inside each handler's transaction.

### 14-02 Frontend API helper + schema
- `orgViewSchema` + `OrgView` type in `schemas.ts`.
- `getOrganization(orgId)` and `updateOrganization(orgId, displayName)`
  in a new `frontend/src/lib/api/organization.ts`.

### 14-03 `/app/settings/general` page + nav item
- Two cards: "Organization" (id, slug, created date) and "Display name"
  (editable inline with save button, inline error, success flash).
- "General" nav item prepended to the Settings group in `+layout.svelte`.
