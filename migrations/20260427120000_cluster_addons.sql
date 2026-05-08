-- =============================================================================
-- 20260427120000_cluster_addons.sql
-- =============================================================================
-- Sprint 2 ticket 07: per-cluster add-on inventory.
--
-- Each row tracks one Helm release on one cluster. The (cluster, addon)
-- pair is unique among live rows so a re-POST with the same body is an
-- idempotent no-op (ticket 07 AC #3).
-- =============================================================================

CREATE TYPE addon_status AS ENUM (
    'pending',     -- row created, install workflow not yet started
    'installing',  -- helm install in flight
    'ready',       -- chart deployed, all replicas ready
    'failed',      -- workflow failed, manual intervention required
    'uninstalling',-- removal workflow in flight (Sprint 3)
    'uninstalled'  -- terminal; row retained for audit
);

CREATE TABLE cluster_addons (
    id              UUID PRIMARY KEY,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    cluster_id      UUID NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,

    -- Catalog slug, e.g. `ingress-nginx`. Validated against the
    -- application-level allowlist before insert.
    addon           TEXT NOT NULL,
    version         TEXT NOT NULL,

    status          addon_status NOT NULL DEFAULT 'pending',
    status_reason   TEXT,
    -- Helm release name actually used inside the cluster — derived
    -- from `addon` today, but stored explicitly so re-targeting later
    -- doesn't break upgrade paths.
    helm_release    TEXT NOT NULL,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ,
    version_lock    BIGINT NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX cluster_addons_cluster_addon_idx
    ON cluster_addons (cluster_id, addon)
    WHERE deleted_at IS NULL;

CREATE INDEX cluster_addons_org_idx ON cluster_addons (organization_id);

ALTER TABLE cluster_addons ENABLE ROW LEVEL SECURITY;
ALTER TABLE cluster_addons FORCE  ROW LEVEL SECURITY;

CREATE POLICY cluster_addons_tenant_scope ON cluster_addons
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());

-- Audit trigger so install / status / uninstall events feed the
-- per-tenant hash chain established in ticket 09.
CREATE TRIGGER cluster_addons_audit
  AFTER INSERT OR UPDATE OR DELETE ON cluster_addons
  FOR EACH ROW EXECUTE FUNCTION audit_log_append_trigger();

-- updated_at trigger uses the existing function from the initial migration.
-- Note: the function references NEW.version, but our column is named
-- version_lock to avoid collision with `addon.version`. Ship a
-- specialised trigger function rather than rewriting the shared one.
CREATE OR REPLACE FUNCTION set_updated_at_with_version_lock() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  NEW.updated_at := now();
  NEW.version_lock := OLD.version_lock + 1;
  RETURN NEW;
END;
$$;

CREATE TRIGGER cluster_addons_updated_at
  BEFORE UPDATE ON cluster_addons
  FOR EACH ROW EXECUTE FUNCTION set_updated_at_with_version_lock();
