# Sprint 12 — Personal API Key Management

## Goal
Let users create, list, and revoke long-lived personal API tokens (`kpat_…`)
from the Security settings page. This sprint ships management only; bearer-token
authentication (accepting `kpat_…` in the `Authorization` header) is deferred
to Sprint 13 to keep the scope tight.

## Tickets

### 12-01 `GET /v1/api-keys` + `POST /v1/api-keys` + `DELETE /v1/api-keys/:id`
- Schema-backed SQL directly in `crates/api/src/api_keys.rs` (same approach as
  `audit_log.rs` — no separate domain service needed for plain CRUD).
- Token format: `kpat_<base64url-32-bytes>` (~256-bit entropy). SHA-256 stored
  in `token_hash BYTEA`; display prefix `kpat_` + first 4 random chars in
  `token_prefix`.
- `POST` returns `{ view, token }` — token is shown once and not stored.
- `DELETE` sets `revoked_at = now()`. Returns 204.
- All three routes gate on a valid session (`Actor` extractor); no owner
  requirement — any member can manage their own keys.
- Tenant scoped via `SET LOCAL app.current_tenant_id` inside every transaction.

### 12-02 Schemas + API helper (`frontend/src/lib/api/api-keys.ts`)
- `apiKeyViewSchema`: `id`, `name`, `token_prefix`, `created_at`, `last_used_at`.
- `createApiKeyResponseSchema`: `{ view: apiKeyViewSchema, token: z.string() }`.
- `listApiKeys()`, `createApiKey(name)`, `revokeApiKey(id)`.

### 12-03 API Keys card on the Security settings page
- Third card below Passkeys and Recovery codes.
- Table: Name / Prefix / Created / Last used / (Revoke button).
- "Create API key" button → modal with `name` input → on success shows the
  `kpat_…` token once with a copy button (same "save these now" pattern as
  recovery codes).
- Empty state with a descriptive message when no keys exist.
