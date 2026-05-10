-- =============================================================================
-- 20260510120000_clusters_agent_liveness.sql
-- =============================================================================
-- Tracks when the kubinate-agent last checked in for each cluster.
--
-- `agent_last_seen_at` is stamped by the control plane on every
-- HeartbeatAck; NULL means the cluster has never had an agent connect.
-- The runbook threshold (> 90 s without a heartbeat fires the alert)
-- is a runtime concern — the schema just stores the raw timestamp.
--
-- `agent_version` is the build version string from the Heartbeat
-- message. Logged for diagnostic purposes; the runbook uses it to
-- correlate stuck agents with known-bad releases.
-- =============================================================================

ALTER TABLE clusters
    ADD COLUMN agent_last_seen_at TIMESTAMPTZ,
    ADD COLUMN agent_version      TEXT NOT NULL DEFAULT '';

-- The agent gRPC service knows the cluster_id (from the Heartbeat message)
-- but not the organization_id; it cannot set `app.current_tenant_id` before
-- the first DB round-trip. This SECURITY DEFINER function runs as the table
-- owner (BYPASSRLS) so it can update any cluster row by id, returning the
-- cluster's organization_id for subsequent cross-checks. The operation is
-- additive-only (writes timestamps/version) and does not leak cross-tenant data.
CREATE OR REPLACE FUNCTION touch_cluster_agent_heartbeat(
    p_cluster_id    uuid,
    p_agent_version text
) RETURNS uuid
LANGUAGE sql SECURITY DEFINER SET search_path = public AS $$
    UPDATE clusters
       SET agent_last_seen_at = now(),
           agent_version      = p_agent_version
     WHERE id          = p_cluster_id
       AND deleted_at  IS NULL
    RETURNING organization_id
$$;

GRANT EXECUTE ON FUNCTION touch_cluster_agent_heartbeat(uuid, text) TO kubinate_app;
