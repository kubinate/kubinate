-- =============================================================================
-- 0001_initial.sql
-- =============================================================================
-- Bootstrap schema for Kubinate: identity, organizations, memberships,
-- sessions, API keys, audit log, and the RLS scaffolding described in
-- ADR-0006.
--
-- Every tenant-scoped table has:
--   * organization_id UUID NOT NULL (FK to organizations)
--   * ENABLE ROW LEVEL SECURITY
--   * FORCE ROW LEVEL SECURITY (applies to table owner too)
--   * A policy that restricts access to rows whose organization_id equals
--     current_setting('app.current_tenant_id')::uuid
--
-- Application code opens every request-scoped transaction with
--   SET LOCAL app.current_tenant_id = '...'
-- via the TenantScopedTransaction wrapper in kubinate-platform.
-- =============================================================================

-- Extensions ------------------------------------------------------------------

CREATE EXTENSION IF NOT EXISTS pgcrypto;    -- envelope encryption (ADR-0007)
CREATE EXTENSION IF NOT EXISTS citext;      -- case-insensitive emails

-- Session variable default ----------------------------------------------------
-- A request that forgets to SET LOCAL app.current_tenant_id will see rows
-- only where organization_id = '00000000-...-0000', which matches nothing.

-- Helper function to read the session tenant id. Returns all-zero UUID when
-- unset, which by construction matches no real row.
CREATE OR REPLACE FUNCTION app_current_tenant_id() RETURNS uuid
LANGUAGE sql STABLE AS $$
  SELECT COALESCE(
    NULLIF(current_setting('app.current_tenant_id', true), ''),
    '00000000-0000-0000-0000-000000000000'
  )::uuid
$$;

-- Organizations ---------------------------------------------------------------

CREATE TABLE organizations (
    id              UUID PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    display_name    TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         BIGINT NOT NULL DEFAULT 1
);

-- Organizations are the tenant boundary itself, so they do not carry an
-- organization_id. RLS for this table is applied at a per-row membership
-- check via the memberships table, expressed in a policy further below.

ALTER TABLE organizations ENABLE ROW LEVEL SECURITY;
ALTER TABLE organizations FORCE  ROW LEVEL SECURITY;

CREATE POLICY organizations_self_scope ON organizations
  USING (id = app_current_tenant_id());

-- Users -----------------------------------------------------------------------
-- Users are global identities; they may belong to many organizations via
-- memberships. Users are NOT tenant-scoped.

CREATE TABLE users (
    id                  UUID PRIMARY KEY,
    email               CITEXT NOT NULL UNIQUE,
    display_name        TEXT NOT NULL,
    avatar_url          TEXT,
    primary_oidc_sub    TEXT,          -- "<issuer>|<subject>" — opaque
    mfa_enrolled        BOOLEAN NOT NULL DEFAULT FALSE,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ,
    version             BIGINT NOT NULL DEFAULT 1
);

CREATE INDEX users_primary_oidc_sub_idx ON users (primary_oidc_sub)
  WHERE primary_oidc_sub IS NOT NULL;

-- OIDC identity link: a user can have multiple linked identities.
CREATE TABLE user_oidc_identities (
    id            UUID PRIMARY KEY,
    user_id       UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    issuer        TEXT NOT NULL,
    subject       TEXT NOT NULL,
    email_at_link CITEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (issuer, subject)
);

CREATE INDEX user_oidc_identities_user_id_idx ON user_oidc_identities (user_id);

-- Memberships -----------------------------------------------------------------

CREATE TYPE membership_role AS ENUM ('owner', 'admin', 'developer', 'viewer');

CREATE TABLE memberships (
    id                UUID PRIMARY KEY,
    organization_id   UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id           UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role              membership_role NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at        TIMESTAMPTZ,
    version           BIGINT NOT NULL DEFAULT 1,
    UNIQUE (organization_id, user_id)
);

CREATE INDEX memberships_user_id_idx ON memberships (user_id);

ALTER TABLE memberships ENABLE ROW LEVEL SECURITY;
ALTER TABLE memberships FORCE  ROW LEVEL SECURITY;

CREATE POLICY memberships_tenant_scope ON memberships
  USING (organization_id = app_current_tenant_id());

-- Sessions --------------------------------------------------------------------

CREATE TABLE sessions (
    id              UUID PRIMARY KEY,              -- opaque session ID
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ NOT NULL,
    revoked_at      TIMESTAMPTZ,
    user_agent      TEXT,
    ip_address      INET
);

CREATE INDEX sessions_user_id_idx ON sessions (user_id);
CREATE INDEX sessions_expires_at_idx ON sessions (expires_at)
  WHERE revoked_at IS NULL;

-- API keys --------------------------------------------------------------------

CREATE TYPE api_key_kind AS ENUM ('personal', 'service_account');

CREATE TABLE api_keys (
    id                UUID PRIMARY KEY,
    organization_id   UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    user_id           UUID REFERENCES users(id) ON DELETE SET NULL,
    kind              api_key_kind NOT NULL,
    name              TEXT NOT NULL,
    token_hash        BYTEA NOT NULL,              -- argon2id of the raw token
    token_prefix      TEXT NOT NULL,               -- e.g. 'kpat_abcd' for display
    scopes            JSONB NOT NULL DEFAULT '[]',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at      TIMESTAMPTZ,
    expires_at        TIMESTAMPTZ,
    revoked_at        TIMESTAMPTZ,
    version           BIGINT NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX api_keys_token_hash_idx ON api_keys (token_hash);
CREATE INDEX api_keys_org_idx ON api_keys (organization_id);

ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys FORCE  ROW LEVEL SECURITY;

CREATE POLICY api_keys_tenant_scope ON api_keys
  USING (organization_id = app_current_tenant_id());

-- Audit log -------------------------------------------------------------------
-- Append-only. Archived to object storage after the hot retention window.

CREATE TABLE audit_log_entries (
    id                UUID PRIMARY KEY,
    organization_id   UUID REFERENCES organizations(id) ON DELETE SET NULL,
    actor_user_id     UUID REFERENCES users(id) ON DELETE SET NULL,
    actor_api_key_id  UUID REFERENCES api_keys(id) ON DELETE SET NULL,
    action            TEXT NOT NULL,
    resource_type     TEXT NOT NULL,
    resource_id       TEXT,
    decision          TEXT NOT NULL,               -- 'allowed' | 'denied'
    reason            TEXT,
    request_id        TEXT,
    ip_address        INET,
    user_agent        TEXT,
    metadata          JSONB NOT NULL DEFAULT '{}',
    prev_hash         BYTEA,                       -- hash-chain, signed
    hash              BYTEA NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX audit_log_entries_org_created_idx
  ON audit_log_entries (organization_id, created_at DESC);
CREATE INDEX audit_log_entries_actor_idx
  ON audit_log_entries (actor_user_id)
  WHERE actor_user_id IS NOT NULL;

ALTER TABLE audit_log_entries ENABLE ROW LEVEL SECURITY;
ALTER TABLE audit_log_entries FORCE  ROW LEVEL SECURITY;

-- Readers see only their tenant; writes go through a bypass role used by
-- the audit pipeline exclusively.
CREATE POLICY audit_log_entries_tenant_read ON audit_log_entries
  FOR SELECT
  USING (organization_id = app_current_tenant_id());

-- updated_at trigger ----------------------------------------------------------

CREATE OR REPLACE FUNCTION set_updated_at() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  NEW.updated_at := now();
  NEW.version   := OLD.version + 1;
  RETURN NEW;
END;
$$;

CREATE TRIGGER organizations_updated_at
  BEFORE UPDATE ON organizations
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER users_updated_at
  BEFORE UPDATE ON users
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER memberships_updated_at
  BEFORE UPDATE ON memberships
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

CREATE TRIGGER api_keys_updated_at
  BEFORE UPDATE ON api_keys
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();
