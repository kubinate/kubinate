-- =============================================================================
-- 20260424120400_auth_states.sql
-- =============================================================================
-- OIDC authorization-request state (Sprint 1 ticket 07).
--
-- When the browser hits `/v1/auth/github/start`, the server generates:
--   * `state`           — opaque anti-CSRF token echoed by the IdP
--   * `nonce`           — binds the response to this request (id_token only)
--   * `code_verifier`   — PKCE proof; SHA-256 of it goes to the IdP as
--                         `code_challenge`
--
-- The triple is stashed here, keyed by `state`, with a tight expiry.
-- On callback the server atomically "consumes" the row (sets
-- consumed_at) — any reuse or expired lookup is a hard auth failure.
-- Pre-auth, so no tenant scope / RLS.
-- =============================================================================

CREATE TABLE auth_states (
    state          TEXT PRIMARY KEY,
    nonce          TEXT NOT NULL,
    code_verifier  TEXT NOT NULL,
    provider       TEXT NOT NULL,            -- 'github' | 'google' | 'microsoft'
    redirect_to    TEXT,                     -- post-login redirect target
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at     TIMESTAMPTZ NOT NULL,
    consumed_at    TIMESTAMPTZ
);

CREATE INDEX auth_states_expires_idx ON auth_states (expires_at);
