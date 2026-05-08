-- =============================================================================
-- 20260424120600_provisioning_workflows.sql
-- =============================================================================
-- Shadow table for the Temporal `ProvisionClusterWorkflow` (ticket 03).
--
-- Temporal is the source of truth for workflow state. This table is a
-- cache optimized for cheap UI queries ("what step is cluster X at?")
-- and for operational dashboards that shouldn't have to hit Temporal's
-- visibility API.
-- =============================================================================

CREATE TYPE provisioning_step AS ENUM (
    'queued',
    'creating_control_plane',
    'waiting_cloud_init_cp',
    'installing_k3s_server',
    'creating_workers',
    'waiting_cloud_init_workers',
    'installing_k3s_agents',
    'collecting_kubeconfig',
    'done',
    'failed'
);

CREATE TABLE provisioning_workflows (
    id                    UUID PRIMARY KEY,
    cluster_id            UUID NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    organization_id       UUID NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    temporal_workflow_id  TEXT,
    temporal_run_id       TEXT,
    current_step          provisioning_step NOT NULL DEFAULT 'queued',
    error_reason          TEXT,
    started_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at           TIMESTAMPTZ,
    version               BIGINT NOT NULL DEFAULT 1
);

CREATE INDEX provisioning_workflows_cluster_idx
    ON provisioning_workflows (cluster_id);
CREATE INDEX provisioning_workflows_org_idx
    ON provisioning_workflows (organization_id);

ALTER TABLE provisioning_workflows ENABLE ROW LEVEL SECURITY;
ALTER TABLE provisioning_workflows FORCE  ROW LEVEL SECURITY;

CREATE POLICY provisioning_workflows_tenant_scope ON provisioning_workflows
  USING       (organization_id = app_current_tenant_id())
  WITH CHECK  (organization_id = app_current_tenant_id());

CREATE TRIGGER provisioning_workflows_updated_at
  BEFORE UPDATE ON provisioning_workflows
  FOR EACH ROW EXECUTE FUNCTION set_updated_at();
