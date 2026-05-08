-- Sprint 4 ticket 05 — short-lived state for in-flight WebAuthn
-- ceremonies (registration + assertion).
--
-- webauthn-rs returns an opaque server-side state from
-- `start_passkey_registration` / `start_passkey_authentication`
-- that must be passed back into the matching `finish_*` call. The
-- state contains the challenge nonce + the user binding; persisting
-- it lets a stateless API instance (multiple replicas in Phase 3+)
-- complete the ceremony without sticky sessions.
--
-- Same architectural shape as `auth_states` from the OAuth flow
-- (see migration 20260424120400_auth_states.sql): a Postgres table
-- with a TTL column. A periodic cleanup is a future ticket; for
-- the alpha load expected in Phase 2/3, the table won't exceed
-- a few thousand live rows.
--
-- Tenancy: like `auth_states`, this is user-scoped, not
-- organization-scoped. Skip RLS.

CREATE TABLE webauthn_ceremonies (
    -- The handle returned to the browser. The browser POSTs it back
    -- to the finish endpoint together with the authenticator's
    -- response.
    id              UUID PRIMARY KEY,
    -- The user the ceremony is for. Set even on registration —
    -- registration ceremonies are only opened for an authenticated
    -- (or partial-MFA-authenticated) user; we never let an
    -- unauthenticated POST register a passkey.
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Discriminator: 'registration' or 'assertion'. Keeps the
    -- finish handlers from accepting cross-kind state.
    kind            TEXT NOT NULL CHECK (kind IN ('registration', 'assertion')),
    -- CBOR-serialised webauthn-rs state. Opaque to the application.
    state           BYTEA NOT NULL,
    -- TTL. webauthn-rs's challenges are short-lived (60s typical);
    -- we set 5 minutes to absorb network jitter without leaving
    -- attack surface open. Cleanup ticket sweeps these.
    expires_at      TIMESTAMPTZ NOT NULL DEFAULT (now() + interval '5 minutes'),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Cleanup-friendly index. The hot path lookup is by id (PK lookup
-- already covered); this serves the periodic-sweep query.
CREATE INDEX webauthn_ceremonies_expires_at_idx
    ON webauthn_ceremonies (expires_at);
