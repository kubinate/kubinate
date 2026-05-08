-- Sprint 4 ticket 05 (deferred row) / Sprint 5 ticket 07: decouple
-- "this user must MFA" from "this user has a passkey to MFA with."
--
-- Today the partial-session decision in
-- `crates/identity/src/session.rs::user_requires_partial_session`
-- inlines `mfa_enrolled = TRUE AND EXISTS (Owner/Admin membership)`.
-- That conflates two distinct invariants:
--
--   1. **Policy**: the user holds an MFA-requiring role somewhere
--      (Owner or Admin in any organization).
--   2. **State**: the user has at least one registered passkey.
--
-- Conflated, the system has no way to express "Owner with no passkey
-- yet" — the partial session is issued, but the SPA can't tell that
-- the user needs to enrol rather than assert. This migration adds
-- `users.requires_mfa` as the policy bit, leaves `mfa_enrolled` as
-- the state bit, and a trigger on `memberships` keeps the policy
-- bit in sync atomically with role mutations.
--
-- The session helper updates separately to read both columns so the
-- four user-facing states (not_required / must_enrol / must_assert
-- / enrolled) are observable.

-- Add the column with a safe default. The trigger below populates
-- it, plus a one-shot backfill at migration time picks up the
-- existing population.
ALTER TABLE users
  ADD COLUMN requires_mfa BOOLEAN NOT NULL DEFAULT FALSE;

-- Backfill: every user who currently holds at least one live
-- Owner-or-Admin membership gets `requires_mfa = TRUE`. Everyone
-- else stays at the column default.
--
-- This runs once at migration time. After this point the trigger
-- below maintains the invariant; the backfill is the bridge from
-- pre-trigger state to trigger-maintained state.
UPDATE users
   SET requires_mfa = TRUE
 WHERE id IN (
   SELECT DISTINCT m.user_id
     FROM memberships m
    WHERE m.deleted_at IS NULL
      AND m.role IN ('owner', 'admin')
 );

-- Trigger function: recompute `users.requires_mfa` for any user
-- whose `memberships` row mutated. Touch a single user per
-- statement so the function stays bounded; bulk role changes (rare)
-- fire the function once per row, which is the right cardinality
-- for a small `memberships` table.
--
-- The recomputation is the same predicate as the backfill: does the
-- user hold at least one live Owner/Admin membership anywhere? A
-- soft-delete (deleted_at NOT NULL) drops the row from the
-- predicate; a role demotion to developer/viewer drops it; a
-- promotion adds it; an entirely new row obviously adds it.
--
-- We don't write to `users.requires_mfa` if the new value matches
-- the existing column value — saves a no-op UPDATE that would
-- otherwise fire the `users` audit trigger and emit a noisy chain
-- entry on every membership write.
CREATE OR REPLACE FUNCTION sync_user_requires_mfa(p_user_id UUID)
RETURNS void
LANGUAGE plpgsql AS $$
DECLARE
  computed BOOLEAN;
  current_value BOOLEAN;
BEGIN
  SELECT EXISTS (
    SELECT 1
      FROM memberships
     WHERE user_id = p_user_id
       AND deleted_at IS NULL
       AND role IN ('owner', 'admin')
  ) INTO computed;

  SELECT requires_mfa
    INTO current_value
    FROM users
   WHERE id = p_user_id;

  IF current_value IS DISTINCT FROM computed THEN
    UPDATE users
       SET requires_mfa = computed
     WHERE id = p_user_id;
  END IF;
END
$$;

-- Trigger wrapper. Fires AFTER each memberships row write so the
-- predicate sees the just-committed state. Handles INSERT, UPDATE
-- (role change OR soft-delete via `deleted_at`), and DELETE
-- (legitimate cascading deletes from `users` ON DELETE CASCADE);
-- DELETE is uncommon in practice — soft-delete via `deleted_at` is
-- the supported path — but we cover it for completeness.
CREATE OR REPLACE FUNCTION memberships_sync_requires_mfa() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
  IF (TG_OP = 'DELETE') THEN
    PERFORM sync_user_requires_mfa(OLD.user_id);
    RETURN OLD;
  END IF;

  -- For UPDATE that changes user_id (extremely unusual, but the
  -- column isn't immutable in the schema), re-sync both sides so
  -- neither user is left with a stale value.
  IF (TG_OP = 'UPDATE' AND NEW.user_id <> OLD.user_id) THEN
    PERFORM sync_user_requires_mfa(OLD.user_id);
  END IF;

  PERFORM sync_user_requires_mfa(NEW.user_id);
  RETURN NEW;
END
$$;

CREATE TRIGGER memberships_requires_mfa_sync
  AFTER INSERT OR UPDATE OR DELETE ON memberships
  FOR EACH ROW EXECUTE FUNCTION memberships_sync_requires_mfa();
