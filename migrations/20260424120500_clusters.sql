-- =============================================================================
-- 20260424120500_clusters.sql
-- =============================================================================
-- Cluster lifecycle domain (Sprint 1 ticket 02 / 03).
--
-- Sprint 1 is the "one user, one cluster" thin slice, so schema is kept
-- conservative. Future migrations extend this with node pools, HA
-- control plane, add-on join rows, etc.
-- =============================================================================

CREATE TYPE cluster_status AS ENUM (
    'pending',       -- form submitted, workflow not yet started
    'provisioning',  -- Temporal workflow running
    'ready',         -- kubeconfig delivered, cluster is usable
    'failed',        -- workflow failed, operator intervention or destroy
    'destroying',    -- tear-down in flight
    'destroyed'      -- terminal; row retained for audit
);

CREATE TABLE clusters (
    id                   UUID PRIMARY KEY,
    organization_id      UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    credential_id        UUID NOT NULL REFERENCES hetzner_credentials(id) ON DELETE RESTRICT,

    name                 TEXT NOT NULL,
    region               TEXT NOT NULL,
    server_type          TEXT NOT NULL,
    control_plane_count  SMALLINT NOT NULL CHECK (control_plane_count >= 1),
    worker_count         SMALLINT NOT NULL CHECK (worker_count >= 1 AND worker_count <= 50),

    status               cluster_status NOT NULL DEFAULT 'pending',
    status_reason        TEXT,                    -- human-readable last-known state
    temporal_workflow_id TEXT,                    -- populated when the workflow starts

    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at           TIMESTAMPTZ,
    version              BIGINT NOT NULL DEFAULT 1
);

CREATE UNIQUE INDEX clusters_org_name_idx
    ON clusters (organization_id, name)
    WHERE deleted_at IS NULL;

CREATE INDEX clusters_org_idx ON clusters (organization_id);
CREATE INDEX clusters_status_idx ON clusters (status) WHERE deleted_at IS NULL;

ALTER TABLE clusters ENABLE ROW LEVEL SECURITY;
ALTER TABLE clusters FORCE  ROW LEVEL SECURITY;

CREATE POLICY clusters_tenant_scope ON clusters
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());

CREATE TRIGGER clusters_updated_at
  BEFORE UPDATE ON clusters
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();

-- Audit trigger (ticket 09 deferred this to the clusters migration).
CREATE TRIGGER clusters_audit
  AFTER INSERT OR UPDATE OR DELETE ON clusters
  FOR EACH ROW EXECUTE FUNCTION audit_log_append_trigger();

-- -----------------------------------------------------------------------------
-- Idempotency-Key store (ticket 02 DoD).
--
-- Keyed by (organization_id, key). A successful POST /v1/clusters caches
-- the (cluster_id, status, body) here; a retried POST with the same key
-- returns the stored response rather than creating a second cluster.
-- -----------------------------------------------------------------------------

CREATE TABLE idempotency_keys (
    organization_id  UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    key              TEXT NOT NULL,
    request_hash     BYTEA NOT NULL,           -- guard against key reuse with a different body
    response_status  SMALLINT NOT NULL,
    response_body    JSONB NOT NULL,
    resource_id      UUID,                     -- e.g. cluster_id, for convenience
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at       TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (organization_id, key)
);

CREATE INDEX idempotency_keys_expires_idx ON idempotency_keys (expires_at);

ALTER TABLE idempotency_keys ENABLE ROW LEVEL SECURITY;
ALTER TABLE idempotency_keys FORCE  ROW LEVEL SECURITY;

CREATE POLICY idempotency_keys_tenant_scope ON idempotency_keys
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());
