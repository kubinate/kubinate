-- =============================================================================
-- 20260424120200_hetzner_credentials.sql
-- =============================================================================
-- Tenant-scoped Hetzner API credentials. The raw token ciphertext lives in
-- `secrets`; this table holds a stable handle + user-facing metadata (alias,
-- timestamps) so the dashboard and orchestrator can reference credentials
-- without ever touching plaintext.
-- =============================================================================

CREATE TABLE hetzner_credentials (
    id              UUID PRIMARY KEY,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    alias           TEXT NOT NULL,
    secret_id       UUID NOT NULL REFERENCES secrets(id) ON DELETE RESTRICT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version         BIGINT NOT NULL DEFAULT 1
);

-- A tenant may have multiple Hetzner tokens (staging, prod), but alias
-- must be unique *among live rows*. Soft-deletion lets us keep an
-- audit trail without blocking reuse of a freed alias.
CREATE UNIQUE INDEX hetzner_credentials_org_alias_idx
    ON hetzner_credentials (organization_id, alias)
    WHERE deleted_at IS NULL;

CREATE INDEX hetzner_credentials_org_idx
    ON hetzner_credentials (organization_id);

ALTER TABLE hetzner_credentials ENABLE ROW LEVEL SECURITY;
ALTER TABLE hetzner_credentials FORCE  ROW LEVEL SECURITY;

CREATE POLICY hetzner_credentials_tenant_scope ON hetzner_credentials
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());
