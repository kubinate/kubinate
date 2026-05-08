# Split the Postgres bootstrap user from the application role

**Labels**: `area/platform`, `area/security`, `sprint-4`
**Epic**: Phase 3 hardening
**Size**: 3

## Context

Postgres superusers **inherently bypass row-level security** even
when the table has `FORCE ROW LEVEL SECURITY` set. CLAUDE.md and
ADR-0006 commit to RLS as the load-bearing tenant-isolation
control, and the platform crate's `secrets.rs` already issues
`SET LOCAL app.current_tenant_id` correctly — but the property
never actually fires when the connection is a superuser.

The Sprint 4 push surfaced this when the workspace tests ran in
CI for the first time:

- `crates/platform/tests/secrets.rs::rls_blocks_cross_tenant_get`
- `crates/platform/tests/secrets.rs::vault_rls_blocks_cross_tenant_get`

Both are now `#[ignore]`'d with a comment naming the gap. They
fail in CI because `docker-compose.yml` + the CI `services.postgres`
both use `POSTGRES_USER: kubinate` as the bootstrap user, which
Postgres treats as a superuser by default. RLS gets bypassed.

In production the API binary won't run as the bootstrap user
(operator-deployed, separate role), so the invariant holds in
prod. But the tests can't prove it without a role split.

## Acceptance criteria

- [ ] A migration creates a `kubinate_app` Postgres role with
      `NOSUPERUSER NOBYPASSRLS NOCREATEROLE NOCREATEDB INHERIT
      LOGIN`.
- [ ] The role has the minimum privileges the application needs:
      `SELECT, INSERT, UPDATE, DELETE` on every existing table,
      `USAGE` on every sequence, `EXECUTE` on
      `app_current_tenant_id` and the audit-trigger functions.
- [ ] Default privileges are set so future tables created by the
      bootstrap user automatically grant the same to
      `kubinate_app`.
- [ ] The application `db::pool` opens connections as
      `kubinate_app`, not as the bootstrap user. Configurable via
      `KUBINATE_DATABASE_URL`'s username field; the bootstrap
      user only runs migrations.
- [ ] The two `#[ignore]`'d tests in
      `crates/platform/tests/secrets.rs` are un-ignored and pass
      in CI.
- [ ] Add a third positive test that asserts the application
      connection's `current_user` is `kubinate_app`, not
      `kubinate` — guards against a future regression that
      forgets to swap roles.
- [ ] CI's `ci.yml` Postgres service is updated so the bootstrap
      step creates `kubinate_app` before migrations run. (Either
      via a `command:` override on the postgres service, or a
      pre-migration step in the workflow.)

## Implementation notes

- The Postgres docker image runs scripts in
  `/docker-entrypoint-initdb.d/` as the bootstrap user; the
  existing `scripts/create-multiple-postgres-databases.sh` lives
  there. Add a sibling that creates `kubinate_app`. Both scripts
  have to be reachable from the CI runner (currently the script
  isn't mounted in CI — only docker-compose mounts it; `ci.yml`
  uses the bare `postgres:16-alpine` service. The role creation
  needs to move to a setup step in the workflow, OR the CI
  service needs a `volumes:` directive. Either works.)
- ADR-0006 should not need an update — the RLS commitment stays.
  This ticket is the missing operational follow-through. A
  paragraph in `docs/runbooks/db-pool-exhaustion.md` (or a new
  runbook entry) documents the role split for operators.

## DoD

- [ ] Migration shipped + CI Postgres bootstrap creates the role.
- [ ] `#[ignore]` removed from the two RLS tests; both pass.
- [ ] One new positive `current_user` assertion test.
- [ ] Production `KUBINATE_DATABASE_URL` example in
      `.env.example` updated to point at `kubinate_app`, not
      `kubinate`.
- [ ] No regression on the existing 63+ workspace tests.

## What this ticket deliberately does **not** do

- **Migrate to a managed Postgres** (CloudNativePG operator).
  That's its own Phase 3 exit gate (per the roadmap); this
  ticket is the role-split groundwork that makes the migration
  cleaner.
- **Drop the bootstrap user's superuser attribute.** Migrations
  still need it (CREATE TABLE / CREATE EXTENSION pgcrypto).
  The split is two-role: bootstrap-superuser for migrations,
  application-non-superuser for runtime.
