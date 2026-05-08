-- Sprint 4 ticket 05 — WebAuthn for Owner / Admin roles.
-- ADR-0009 §MFA commits to "WebAuthn (passkeys) supported for
-- interactive sessions. Enforced for accounts with Admin or Owner
-- roles in Phase 3+."
--
-- Three changes:
--   1. user_passkeys      — registered passkeys per user.
--   2. mfa_recovery_codes — one-shot codes for the lost-device path.
--   3. sessions.mfa_satisfied — partial-session marker.
--
-- Tenancy note: passkeys + recovery codes are USER-scoped, not
-- tenant-scoped. A user with `Owner` in org A and `Member` in org B
-- has one set of credentials. Per ADR-0006 the RLS pattern only
-- applies to tenant-scoped tables; these aren't, so they stay
-- unscoped (same as `users` and `sessions`).
--
-- Audit: a per-event audit log for "passkey enrolled / used /
-- revoked" is a follow-up ticket — the security DoD row in
-- `docs/backlog/sprint-4/05-webauthn-for-owners.md` calls for it,
-- but the storage shape doesn't gate on the trigger landing first.

CREATE TABLE user_passkeys (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The webauthn-rs crate stores the credential id as a Base64URL
    -- string; we keep that representation so a stolen DB row can be
    -- compared against the wire shape without re-encoding.
    credential_id   TEXT NOT NULL,
    -- The webauthn-rs `Passkey` blob (CBOR-encoded). Opaque to the
    -- application — we hand it back to webauthn-rs verbatim during
    -- assertion ceremony.
    credential      BYTEA NOT NULL,
    -- Sign counter. webauthn-rs detects clones by rejecting an
    -- assertion whose counter is <= stored. We persist the highest
    -- counter we've seen.
    sign_counter    BIGINT NOT NULL DEFAULT 0,
    -- User-supplied label ("YubiKey 5C"). Capped at 64 chars
    -- application-side; no DB constraint to keep the ALTER trivial.
    nickname        TEXT NOT NULL,
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at    TIMESTAMPTZ,
    -- Soft-delete: a revoked passkey can't be re-used and its row
    -- sticks around for the audit story we'll wire later.
    revoked_at      TIMESTAMPTZ
);

CREATE INDEX user_passkeys_user_id_idx ON user_passkeys (user_id)
    WHERE revoked_at IS NULL;
-- A credential id must be globally unique (it's the WebAuthn-level
-- handle the authenticator returns). Partial-unique on live rows so
-- a revoked passkey can be re-registered if the user wants to.
CREATE UNIQUE INDEX user_passkeys_credential_id_unique
    ON user_passkeys (credential_id) WHERE revoked_at IS NULL;

CREATE TABLE mfa_recovery_codes (
    id              UUID PRIMARY KEY,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- We store the SHA-256 hash of the code, never the plaintext.
    -- The user sees the codes once at enrollment + can regenerate
    -- the batch (which invalidates every prior code).
    code_hash       BYTEA NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One-shot: set the moment the code is consumed; subsequent
    -- attempts to use it must fail.
    used_at         TIMESTAMPTZ
);

CREATE INDEX mfa_recovery_codes_user_id_idx ON mfa_recovery_codes (user_id)
    WHERE used_at IS NULL;
-- Hash collisions across users are vanishingly unlikely but the
-- partial-unique on the hash itself keeps a deterministic-RNG bug
-- from silently producing two equal codes. Live rows only.
CREATE UNIQUE INDEX mfa_recovery_codes_hash_unique
    ON mfa_recovery_codes (code_hash) WHERE used_at IS NULL;

-- Existing sessions stay valid: the column defaults to TRUE so any
-- session issued before this migration is treated as MFA-satisfied.
-- New sessions for users on the MFA-required path (Owner / Admin
-- roles, set application-side at issue time) explicitly write FALSE
-- and only flip to TRUE after the WebAuthn assertion succeeds.
ALTER TABLE sessions
    ADD COLUMN mfa_satisfied BOOLEAN NOT NULL DEFAULT TRUE;
