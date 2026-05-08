-- =============================================================================
-- 20260424120700_kubeconfigs.sql
-- =============================================================================
-- Cluster kubeconfig storage (Sprint 1 ticket 04).
--
-- Stores the kubeconfig produced by the provisioning workflow as a
-- ciphertext blob in `secrets`, with the cluster row pointing at the
-- secret via `kubeconfig_secret_id`. Reads are cleartext only inside
-- the API handler scope (threat-model Flow 3).
--
-- Also adds `audit_log_append_explicit`, a callable function for
-- non-DML audit events (e.g. `kubeconfig.retrieved`) that the
-- per-table trigger cannot capture — a SELECT does not fire INSERT
-- /UPDATE/DELETE triggers.
-- =============================================================================

ALTER TABLE clusters
  ADD COLUMN kubeconfig_secret_id UUID REFERENCES secrets(id) ON DELETE RESTRICT;

CREATE INDEX clusters_kubeconfig_secret_idx
  ON clusters (kubeconfig_secret_id)
  WHERE kubeconfig_secret_id IS NOT NULL;

-- -----------------------------------------------------------------------------
-- Explicit-append audit function for non-DML events.
--
-- Mirrors what `audit_log_append_trigger` does, but takes the event
-- shape as parameters so the application layer can record reads
-- (`kubeconfig.retrieved`), authentication outcomes, etc. Reuses the
-- same `audit_log_hash_input` so chain verification stays consistent
-- regardless of how a row got written.
-- -----------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION audit_log_append_explicit(
    p_org           uuid,
    p_action        text,
    p_resource_type text,
    p_resource_id   text,
    p_decision      text DEFAULT 'allowed',
    p_metadata      jsonb DEFAULT '{}'::jsonb
) RETURNS uuid
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public AS $$
DECLARE
  v_id   uuid;
  v_prev bytea;
  v_hash bytea;
BEGIN
  SELECT hash INTO v_prev
  FROM audit_log_entries
  WHERE organization_id = p_org
  ORDER BY created_at DESC, id DESC
  LIMIT 1
  FOR UPDATE;

  v_hash := digest(
    audit_log_hash_input(v_prev, p_org, p_action, p_resource_type, p_resource_id),
    'sha256'
  );

  v_id := gen_random_uuid();
  INSERT INTO audit_log_entries (
    id, organization_id, actor_user_id, action, resource_type, resource_id,
    decision, request_id, ip_address, user_agent, metadata, prev_hash, hash
  ) VALUES (
    v_id,
    p_org,
    app_setting_uuid('app.current_actor_id'),
    p_action,
    p_resource_type,
    p_resource_id,
    p_decision,
    app_setting_text('app.current_request_id'),
    app_setting_inet('app.current_request_ip'),
    app_setting_text('app.current_user_agent'),
    p_metadata,
    v_prev,
    v_hash
  );

  RETURN v_id;
END;
$$;
