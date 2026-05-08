-- =============================================================================
-- 20260424120300_audit_log.sql
-- =============================================================================
-- Hash-chained audit log (Sprint 1 ticket 09).
--
-- The `audit_log_entries` table already exists from the initial migration.
-- This migration adds:
--   * a generic AFTER-each-row trigger function that appends one audit
--     entry per mutation, chained to the previous entry for the tenant;
--   * a verification function that walks the chain and returns rows
--     whose hash no longer matches the stored `prev_hash` / contents;
--   * an INSERT RLS policy so trigger-originated writes pass.
--
-- Triggers are attached to `hetzner_credentials` here. `sessions` has no
-- `organization_id` (user-scoped, cross-tenant) and is addressed in the
-- session-issuance ticket 07. `clusters` does not exist yet and is
-- addressed in ticket 03's migration.
--
-- Session variable contract for callers (set via SET LOCAL):
--   app.current_tenant_id   -- UUID, required (already used by RLS)
--   app.current_actor_id    -- UUID, optional (no user bound → NULL)
--   app.current_request_id  -- text, optional
--   app.current_request_ip  -- inet text, optional
--   app.current_user_agent  -- text, optional
-- =============================================================================

-- Small helpers to read typed session variables. The `true` argument to
-- current_setting makes it return '' for an unset GUC instead of erroring.
CREATE OR REPLACE FUNCTION app_setting_text(name text) RETURNS text
LANGUAGE sql STABLE AS $$
  SELECT NULLIF(current_setting(name, true), '')
$$;

CREATE OR REPLACE FUNCTION app_setting_uuid(name text) RETURNS uuid
LANGUAGE sql STABLE AS $$
  SELECT NULLIF(current_setting(name, true), '')::uuid
$$;

CREATE OR REPLACE FUNCTION app_setting_inet(name text) RETURNS inet
LANGUAGE sql STABLE AS $$
  SELECT NULLIF(current_setting(name, true), '')::inet
$$;

-- Canonical hash input. Keeping the serialization format here (not in
-- Rust) means the verification function sees exactly the same bytes the
-- trigger hashed, regardless of which client wrote the row.
CREATE OR REPLACE FUNCTION audit_log_hash_input(
    p_prev bytea,
    p_org uuid,
    p_action text,
    p_resource_type text,
    p_resource_id text
) RETURNS bytea
LANGUAGE sql IMMUTABLE AS $$
  SELECT COALESCE(p_prev, ''::bytea)
      || convert_to(p_org::text, 'UTF8') || convert_to('|', 'UTF8')
      || convert_to(p_action, 'UTF8')    || convert_to('|', 'UTF8')
      || convert_to(p_resource_type, 'UTF8') || convert_to('|', 'UTF8')
      || convert_to(p_resource_id, 'UTF8')
$$;

-- Generic append trigger. Run SECURITY DEFINER so the insert into
-- `audit_log_entries` passes the SELECT-only RLS on that table — the
-- definer owns the table and can INSERT. The INSERT RLS policy below
-- is still required because FORCE ROW LEVEL SECURITY applies to owners
-- too.
CREATE OR REPLACE FUNCTION audit_log_append_trigger() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public AS $$
DECLARE
  v_org      uuid;
  v_resource text;
  v_action   text;
  v_prev     bytea;
  v_hash     bytea;
BEGIN
  IF TG_OP = 'DELETE' THEN
    v_org      := OLD.organization_id;
    v_resource := OLD.id::text;
    v_action   := TG_TABLE_NAME || '.deleted';
  ELSE
    v_org      := NEW.organization_id;
    v_resource := NEW.id::text;
    v_action   := CASE TG_OP
                    WHEN 'INSERT' THEN TG_TABLE_NAME || '.created'
                    WHEN 'UPDATE' THEN TG_TABLE_NAME || '.updated'
                  END;
  END IF;

  -- Serialize hash-chain appends per tenant. FOR UPDATE of the tail row
  -- forces concurrent audits for the same tenant into a linear order
  -- (otherwise two appenders could read the same prev_hash and produce
  -- a fork).
  SELECT hash INTO v_prev
  FROM audit_log_entries
  WHERE organization_id = v_org
  ORDER BY created_at DESC, id DESC
  LIMIT 1
  FOR UPDATE;

  v_hash := digest(
    audit_log_hash_input(v_prev, v_org, v_action, TG_TABLE_NAME, v_resource),
    'sha256'
  );

  INSERT INTO audit_log_entries (
    id, organization_id, actor_user_id, action, resource_type, resource_id,
    decision, request_id, ip_address, user_agent, metadata, prev_hash, hash
  ) VALUES (
    gen_random_uuid(),
    v_org,
    app_setting_uuid('app.current_actor_id'),
    v_action,
    TG_TABLE_NAME,
    v_resource,
    'allowed',
    app_setting_text('app.current_request_id'),
    app_setting_inet('app.current_request_ip'),
    app_setting_text('app.current_user_agent'),
    '{}'::jsonb,
    v_prev,
    v_hash
  );

  IF TG_OP = 'DELETE' THEN RETURN OLD; END IF;
  RETURN NEW;
END;
$$;

-- Verification walks the chain in insertion order and returns any row
-- whose stored prev_hash or hash disagrees with the recomputed values.
CREATE OR REPLACE FUNCTION audit_log_verify(p_org uuid)
RETURNS TABLE(broken_id uuid, reason text)
LANGUAGE plpgsql AS $$
DECLARE
  r RECORD;
  v_prev     bytea := NULL;
  v_expected bytea;
BEGIN
  FOR r IN
    SELECT id, organization_id, action, resource_type, resource_id,
           prev_hash, hash
    FROM audit_log_entries
    WHERE organization_id = p_org
    ORDER BY created_at ASC, id ASC
  LOOP
    IF r.prev_hash IS DISTINCT FROM v_prev THEN
      broken_id := r.id;
      reason    := 'prev_hash mismatch';
      RETURN NEXT;
    END IF;

    v_expected := digest(
      audit_log_hash_input(v_prev, r.organization_id, r.action,
                           r.resource_type, r.resource_id),
      'sha256'
    );
    IF r.hash IS DISTINCT FROM v_expected THEN
      broken_id := r.id;
      reason    := 'hash mismatch';
      RETURN NEXT;
    END IF;

    v_prev := r.hash;
  END LOOP;
  RETURN;
END;
$$;

-- Allow trigger-originated inserts past RLS. The SELECT policy from
-- the initial migration still scopes reads to the caller's tenant;
-- this new policy only opens up INSERT. Production sets a restrictive
-- role check here (`WITH CHECK (current_user = 'audit_writer')`); for
-- Phase 0 every mutation goes through the trigger, which is the only
-- code path that inserts, so an unconditional `true` is acceptable.
CREATE POLICY audit_log_entries_trigger_write ON audit_log_entries
  FOR INSERT
  WITH CHECK (true);

-- Attach the trigger to the tables whose mutations ticket 01 / 09 want
-- audited. `clusters` and `sessions` get their own CREATE TRIGGER in
-- their respective feature migrations.
CREATE TRIGGER hetzner_credentials_audit
  AFTER INSERT OR UPDATE OR DELETE ON hetzner_credentials
  FOR EACH ROW EXECUTE FUNCTION audit_log_append_trigger();
