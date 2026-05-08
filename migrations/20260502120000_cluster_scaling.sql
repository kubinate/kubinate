-- =============================================================================
-- 20260502120000_cluster_scaling.sql
-- =============================================================================
-- Sprint 3 ticket 08: scale-out / scale-in workflows.
--
-- The cluster_status enum gets a new in-flight value so the dashboard
-- can render "scaling" without ambiguity (the workflow returns to
-- `ready` on success, `failed` on error).
-- =============================================================================

ALTER TYPE cluster_status ADD VALUE IF NOT EXISTS 'scaling';
