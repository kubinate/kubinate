-- =============================================================================
-- 20260426120000_invites_and_memberships.sql
-- =============================================================================
-- Sprint 2 ticket 06: multi-org membership management.
--
-- Adds:
--   * `invites` table — pending invitations with single-use, argon2id-hashed
--     tokens and a 7-day default TTL.
--   * Audit triggers on `memberships` and `invites` so role changes,
--     invites, accepts, and revocations are part of the per-tenant
--     hash chain (ticket 09 deferred these to the consuming feature).
-- =============================================================================

CREATE TABLE invites (
    id              UUID PRIMARY KEY,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    email           CITEXT NOT NULL,
    role            membership_role NOT NULL,

    -- Argon2id hash of the raw token. Same shape as `api_keys.token_hash`
    -- so the audit / forensics tooling treats both uniformly.
    token_hash      BYTEA NOT NULL,
    -- Display-only prefix, e.g. `kinv_abcd…`. Surfaced in the UI so an
    -- owner can identify which invite a recipient is asking about
    -- without ever seeing the secret half.
    token_prefix    TEXT  NOT NULL,

    invited_by      UUID REFERENCES users(id) ON DELETE SET NULL,
    expires_at      TIMESTAMPTZ NOT NULL,
    accepted_at     TIMESTAMPTZ,
    accepted_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    revoked_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    version         BIGINT NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX invites_token_hash_idx ON invites (token_hash);
CREATE INDEX invites_org_idx ON invites (organization_id);
-- Partial unique on (org, email) restricted to pending invites stops an
-- owner from accidentally double-inviting the same address; once
-- accepted or revoked, the row stays for audit and a fresh invite can
-- be issued.
CREATE UNIQUE INDEX invites_org_email_pending_idx
    ON invites (organization_id, email)
    WHERE accepted_at IS NULL AND revoked_at IS NULL;

ALTER TABLE invites ENABLE ROW LEVEL SECURITY;
ALTER TABLE invites FORCE  ROW LEVEL SECURITY;

CREATE POLICY invites_tenant_scope ON invites
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());

CREATE TRIGGER invites_updated_at
  BEFORE UPDATE ON invites
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Audit triggers — `memberships` and `invites` both feed the
-- per-tenant hash chain established in ticket 09.
CREATE TRIGGER memberships_audit
  AFTER INSERT OR UPDATE OR DELETE ON memberships
  FOR EACH ROW EXECUTE FUNCTION audit_log_append_trigger();

CREATE TRIGGER invites_audit
  AFTER INSERT OR UPDATE OR DELETE ON invites
  FOR EACH ROW EXECUTE FUNCTION audit_log_append_trigger();
