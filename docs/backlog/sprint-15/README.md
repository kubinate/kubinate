# Sprint 15 — Personal Profile Settings

## Goal
Let authenticated users update their own display name without going through
an admin. Closes the last personal-settings gap now that org display name
(Sprint 14) and security settings (Sprint 4) are both done.

## Tickets

### 15-01 `PATCH /v1/me`
- Accepts `{ display_name }`, trims, validates 1–100 chars.
- Returns the full `MeView` (same shape as `GET /v1/me`).
- Any authenticated `Actor` — no role gate.
- Added to the `/v1/me` route alongside the existing `GET`.

### 15-02 Frontend API helper
- `updateMe(displayName)` in `frontend/src/lib/api/me.ts`.
- Reuses `meViewSchema` + `MeView` type already in `schemas.ts`.

### 15-03 `/app/settings/profile` page + nav item
- Two cards: "Account" (read-only email + user id) and "Display name"
  (editable inline with save button, inline error, success flash).
- "Profile" nav item added first in the Settings group in `+layout.svelte`.
