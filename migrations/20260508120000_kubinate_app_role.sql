-- Sprint 4 ticket 07 — split bootstrap from runtime DB role.
--
-- Postgres superusers inherently bypass row-level security even with
-- `FORCE ROW LEVEL SECURITY` set. The Phase 0 docker-compose +
-- CI workflows have always run the API as the bootstrap user (which
-- is implicitly a superuser when created via `POSTGRES_USER`), so
-- the cross-tenant isolation invariant from ADR-0006 has been
-- enforced by application code only — RLS in CI was a placebo.
--
-- This migration creates a NOSUPERUSER NOBYPASSRLS role
-- `kubinate_app` and grants it the minimum runtime privileges. The
-- API binary will connect as this role at runtime; migrations
-- continue to run as the bootstrap user (which now has the only
-- DDL-issuing privilege on the schema). The two RLS isolation
-- tests in `crates/platform/tests/secrets.rs` un-ignore once the
-- pool routes through the right role.
--
-- See `docs/backlog/sprint-4/07-postgres-role-split.md` for the
-- ticket-level reasoning.

-- Wrap the CREATE ROLE in a DO block so re-running this migration
-- against a database where the role already exists (e.g. an
-- operator running `sqlx migrate run` after manually pre-creating
-- the role) doesn't fail.
DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kubinate_app') THEN
    CREATE ROLE kubinate_app
      LOGIN
      NOSUPERUSER
      NOBYPASSRLS
      NOCREATEDB
      NOCREATEROLE
      NOREPLICATION
      INHERIT;
  END IF;
END
$$;

-- The default password is set via the docker-compose init script
-- (`scripts/create-multiple-postgres-databases.sh`) and the CI
-- workflow's setup step. Do not embed a password in this migration
-- — operators rotate it independent of the schema lifecycle.

-- Schema usage. `public` is the only schema we use today.
GRANT USAGE ON SCHEMA public TO kubinate_app;

-- Existing tables: full DML. The bootstrap user remains the owner;
-- DDL stays with the bootstrap user, so DROP/ALTER cannot be issued
-- by a compromised application connection.
GRANT SELECT, INSERT, UPDATE, DELETE
  ON ALL TABLES IN SCHEMA public TO kubinate_app;

-- Sequences (UUID v7 keys don't need them today, but `bigserial`
-- columns and any future identity columns will).
GRANT USAGE, SELECT, UPDATE
  ON ALL SEQUENCES IN SCHEMA public TO kubinate_app;

-- Functions: the audit triggers + `app_current_tenant_id` helper
-- need EXECUTE for tenant-scoped DML to work.
GRANT EXECUTE
  ON ALL FUNCTIONS IN SCHEMA public TO kubinate_app;

-- Default privileges for tables created **by the bootstrap role
-- in the future** — every subsequent migration runs as the
-- bootstrap user, so this is what binds the role to the schema.
ALTER DEFAULT PRIVILEGES FOR ROLE CURRENT_USER IN SCHEMA public
  GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO kubinate_app;
ALTER DEFAULT PRIVILEGES FOR ROLE CURRENT_USER IN SCHEMA public
  GRANT USAGE, SELECT, UPDATE ON SEQUENCES TO kubinate_app;
ALTER DEFAULT PRIVILEGES FOR ROLE CURRENT_USER IN SCHEMA public
  GRANT EXECUTE ON FUNCTIONS TO kubinate_app;
