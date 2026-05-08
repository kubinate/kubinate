-- =============================================================================
-- 20260424120800_cluster_servers.sql
-- =============================================================================
-- Per-cluster Hetzner server inventory (Sprint 1 ticket 08).
--
-- The destroy workflow needs to know which servers it created. Without
-- this table we'd have to query Hetzner with a label filter — fragile
-- and orphan-prone. Each provisioning run records every server it
-- creates here in the same transaction as it spawns the workflow step
-- so cleanup can never miss a row.
-- =============================================================================

CREATE TYPE cluster_node_role AS ENUM ('control_plane', 'worker');

CREATE TABLE cluster_servers (
    id              UUID PRIMARY KEY,
    organization_id UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    cluster_id      UUID NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    -- Hetzner ids are i64 in the API; store as BIGINT to match.
    hetzner_server_id  BIGINT NOT NULL,
    role            cluster_node_role NOT NULL,
    public_ipv4     INET,
    private_ipv4    INET,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX cluster_servers_hetzner_id_idx
    ON cluster_servers (hetzner_server_id)
    WHERE deleted_at IS NULL;

CREATE INDEX cluster_servers_cluster_idx ON cluster_servers (cluster_id);

ALTER TABLE cluster_servers ENABLE ROW LEVEL SECURITY;
ALTER TABLE cluster_servers FORCE  ROW LEVEL SECURITY;

CREATE POLICY cluster_servers_tenant_scope ON cluster_servers
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());
