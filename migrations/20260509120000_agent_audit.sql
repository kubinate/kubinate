-- =============================================================================
-- 20260509120000_agent_audit.sql
-- =============================================================================
-- Per-cluster agent audit chain (Sprint 5 ticket 05).
--
-- The `agent_audit` table records every significant event on an agent
-- tunnel connection: connect, disconnect, heartbeat gap, cert rotation,
-- and RPC dispatch. The chain is per-cluster (keyed on `cluster_id`),
-- not per-tenant, because a cluster's audit trail needs to be portable
-- even if the organization is deleted or the cluster is transferred.
--
-- Hash-chain shape mirrors `audit_log_entries` (Sprint 1 ticket 09):
--   prev_hash ← sha256(prev_hash || cluster_id || action || resource_id)
-- This means the chain verifier in `audit_log_verify` can be adapted
-- for agent_audit by substituting the cluster-scoped columns.
--
-- The trigger function `agent_audit_append_trigger` fires AFTER INSERT
-- on this table; it fills `prev_hash` and `hash` automatically.
-- Callers INSERT rows with `prev_hash = NULL` and `hash = NULL`; the
-- trigger computes both before the row lands.
-- =============================================================================

CREATE TABLE agent_audit (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    cluster_id      UUID        NOT NULL REFERENCES clusters(id) ON DELETE CASCADE,
    organization_id UUID        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
    action          TEXT        NOT NULL,
    resource_id     TEXT        NOT NULL DEFAULT '',
    metadata        JSONB       NOT NULL DEFAULT '{}',
    actor_ip        INET,
    prev_hash       BYTEA,
    hash            BYTEA       NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX agent_audit_cluster_idx ON agent_audit (cluster_id, created_at DESC);
CREATE INDEX agent_audit_org_idx     ON agent_audit (organization_id, created_at DESC);

-- Hash input mirrors the audit_log pattern; cluster_id scopes the chain.
CREATE OR REPLACE FUNCTION agent_audit_hash_input(
    p_prev       bytea,
    p_cluster_id uuid,
    p_action     text,
    p_resource   text
) RETURNS bytea
LANGUAGE sql IMMUTABLE AS $$
  SELECT COALESCE(p_prev, ''::bytea)
      || convert_to(p_cluster_id::text, 'UTF8') || convert_to('|', 'UTF8')
      || convert_to(p_action,           'UTF8') || convert_to('|', 'UTF8')
      || convert_to(p_resource,         'UTF8')
$$;

-- Append trigger: fills prev_hash + hash on INSERT. Serialises per-cluster
-- appends with FOR UPDATE on the tail row to prevent chain forks.
CREATE OR REPLACE FUNCTION agent_audit_append_trigger() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public AS $$
DECLARE
  v_prev bytea;
  v_hash bytea;
BEGIN
  SELECT hash INTO v_prev
  FROM agent_audit
  WHERE cluster_id = NEW.cluster_id
  ORDER BY created_at DESC, id DESC
  LIMIT 1
  FOR UPDATE;

  v_hash := digest(
    agent_audit_hash_input(v_prev, NEW.cluster_id, NEW.action, NEW.resource_id),
    'sha256'
  );

  NEW.prev_hash := v_prev;
  NEW.hash      := v_hash;
  RETURN NEW;
END;
$$;

CREATE TRIGGER agent_audit_chain
  BEFORE INSERT ON agent_audit
  FOR EACH ROW EXECUTE FUNCTION agent_audit_append_trigger();

-- Chain verifier — same shape as audit_log_verify.
CREATE OR REPLACE FUNCTION agent_audit_verify(p_cluster_id uuid)
RETURNS TABLE(broken_id uuid, reason text)
LANGUAGE plpgsql AS $$
DECLARE
  r      RECORD;
  v_prev bytea := NULL;
  v_exp  bytea;
BEGIN
  FOR r IN
    SELECT id, cluster_id, action, resource_id, prev_hash, hash
    FROM agent_audit
    WHERE cluster_id = p_cluster_id
    ORDER BY created_at ASC, id ASC
  LOOP
    IF r.prev_hash IS DISTINCT FROM v_prev THEN
      broken_id := r.id; reason := 'prev_hash mismatch'; RETURN NEXT;
    END IF;
    v_exp := digest(
      agent_audit_hash_input(v_prev, r.cluster_id, r.action, r.resource_id),
      'sha256'
    );
    IF r.hash IS DISTINCT FROM v_exp THEN
      broken_id := r.id; reason := 'hash mismatch'; RETURN NEXT;
    END IF;
    v_prev := r.hash;
  END LOOP;
  RETURN;
END;
$$;
