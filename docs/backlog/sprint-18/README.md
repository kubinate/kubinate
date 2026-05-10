# Sprint 18 — Team Page Polish

## Goal
Surface the data the backend already tracks but the team page never showed,
and give admins a one-click way to re-issue an expired invite.

## Tickets

### 18-01 "Last active" column on members table
- `last_active_at` is already on `MembershipView` (populated by the session
  activity bump shipped in Sprint 16). The members table now shows it as a
  localized date, or "—" when null.

### 18-02 Invite status badge column
- New "Status" column between Role and Expires shows a colour-coded badge:
  blue = pending, amber = expired, emerald = accepted, outline = revoked.
- Moves status out of the Actions cell so the table is easier to scan.

### 18-03 Resend invite button
- "Resend" appears on pending and expired invites.
- For pending invites: revokes the existing invite first, then creates a new
  one with the same email + role (avoiding a duplicate-invite conflict).
- For expired invites: creates a new invite directly.
- On success the new token URL banner appears, identical to the initial
  invite flow. MFA redirect and error handling match existing patterns.
