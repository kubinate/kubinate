-- =============================================================================
-- 20260429120000_organizations_audit.sql
-- =============================================================================
-- Sprint 3 ticket 06: column-aware audit trigger on `organizations`.
--
-- The Sprint 1 ticket 09 generic trigger writes an audit row for every
-- DML on every attached table. `organizations` updates happen all the
-- time (display_name edits, slug renames, future feature flags) and we
-- don't want a flood of audit rows for changes that aren't billing-
-- relevant.
--
-- This trigger fires on UPDATE only, short-circuits when neither
-- `plan` nor `stripe_customer_id` changed, and writes a chain-valid
-- audit row with the before/after values in `metadata`. Reuses
-- `audit_log_hash_input` so the chain format stays identical.
-- =============================================================================

CREATE OR REPLACE FUNCTION organizations_billing_audit_trigger() RETURNS trigger
LANGUAGE plpgsql SECURITY DEFINER SET search_path = public AS $$
DECLARE
  v_prev   bytea;
  v_hash   bytea;
  v_action text;
BEGIN
  -- Only emit when a billing-relevant column actually changed.
  -- IS DISTINCT FROM handles NULL transitions correctly.
  IF NEW.plan IS NOT DISTINCT FROM OLD.plan
     AND NEW.stripe_customer_id IS NOT DISTINCT FROM OLD.stripe_customer_id THEN
    RETURN NEW;
  END IF;

  v_action := 'organizations.updated';

  -- For the `organizations` table the row IS the tenant boundary, so
  -- `organization_id` on the audit row is the row's own id rather than
  -- `app.current_tenant_id` (which may not even be set in this path).
  SELECT hash INTO v_prev
  FROM audit_log_entries
  WHERE organization_id = NEW.id
  ORDER BY created_at DESC, id DESC
  LIMIT 1
  FOR UPDATE;

  v_hash := digest(
    audit_log_hash_input(v_prev, NEW.id, v_action, TG_TABLE_NAME, NEW.id::text),
    'sha256'
  );

  INSERT INTO audit_log_entries (
    id, organization_id, actor_user_id, action, resource_type, resource_id,
    decision, request_id, ip_address, user_agent, metadata, prev_hash, hash
  ) VALUES (
    gen_random_uuid(),
    NEW.id,
    app_setting_uuid('app.current_actor_id'),
    v_action,
    TG_TABLE_NAME,
    NEW.id::text,
    'allowed',
    app_setting_text('app.current_request_id'),
    app_setting_inet('app.current_request_ip'),
    app_setting_text('app.current_user_agent'),
    jsonb_build_object(
      'old_plan',                OLD.plan::text,
      'new_plan',                NEW.plan::text,
      'old_stripe_customer_id',  OLD.stripe_customer_id,
      'new_stripe_customer_id',  NEW.stripe_customer_id
    ),
    v_prev,
    v_hash
  );

  RETURN NEW;
END;
$$;

CREATE TRIGGER organizations_billing_audit
  AFTER UPDATE OF plan, stripe_customer_id ON organizations
  FOR EACH ROW EXECUTE FUNCTION organizations_billing_audit_trigger();
