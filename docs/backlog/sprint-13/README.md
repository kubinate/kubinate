# Sprint 13 — API Key Bearer Token Authentication

## Goal
Wire the `kpat_…` tokens created in Sprint 12 as a real authentication
mechanism. After this sprint, any endpoint that currently accepts the
session cookie also accepts `Authorization: Bearer kpat_…`.

## Tickets

### 13-01 RLS bypass policy for cross-tenant auth lookups
- New migration `20260510180000_api_keys_auth_bypass.sql`.
- Permissive SELECT policy that fires when the caller sets
  `SET LOCAL app.api_key_auth_bypass = 'on'` inside a transaction.
- This allows the API to find a key by its SHA-256 hash before knowing
  which tenant the key belongs to, without exposing cross-tenant data
  under normal operation.

### 13-02 Bearer token path in `Actor` extractor
- After the cookie path and before the dev-header fallback, check for
  `Authorization: Bearer kpat_...`.
- Hash the token (SHA-256), SET LOCAL the bypass, query `api_keys`,
  update `last_used_at` in the same transaction.
- Returns `Actor { user_id, organization_id, session_id: Uuid::nil() }`
  — `session_id.is_nil()` signals API key auth and skips the MFA gate
  in `OwnerActor` (same convention as the dev-header fallback).

### 13-03 Frontend: API key usage hint on the Security page
- Show the token prefix + a copy-ready curl snippet in the one-time
  token display modal after a key is created.
- Minor: "Last used" timestamp on the key list refreshes on next page
  load (no polling needed — the list is fetched on mount).
