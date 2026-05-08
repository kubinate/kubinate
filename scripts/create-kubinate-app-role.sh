#!/usr/bin/env bash
# Sprint 4 ticket 07 — bootstrap the runtime application role.
#
# Runs as part of the Postgres image's `/docker-entrypoint-initdb.d/`
# pass on first start, **before** sqlx migrations run. Creates a
# `kubinate_app` role with NOSUPERUSER NOBYPASSRLS so the runtime
# API connection actually triggers row-level security.
#
# The role's password is taken from `KUBINATE_APP_DB_PASSWORD` (env)
# with a hard-coded fallback for `docker compose up` ergonomics —
# the fallback is identical to the bootstrap password, which is
# already a dev-only value committed in `docker-compose.yml`.
# Production deploys set `KUBINATE_APP_DB_PASSWORD` to a real
# secret via Ansible Vault (Phase 2) or HashiCorp Vault dynamic
# credentials (Phase 3+, see ADR-0007).
#
# Idempotent: re-runs are no-ops because the migration that grants
# privileges does its own `IF NOT EXISTS` check and Postgres init
# scripts only fire on first start.

set -euo pipefail

APP_PASSWORD="${KUBINATE_APP_DB_PASSWORD:-kubinate}"

psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" <<-EOSQL
  DO \$\$
  BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kubinate_app') THEN
      CREATE ROLE kubinate_app
        LOGIN
        NOSUPERUSER
        NOBYPASSRLS
        NOCREATEDB
        NOCREATEROLE
        NOREPLICATION
        INHERIT
        PASSWORD '$APP_PASSWORD';
    ELSE
      ALTER ROLE kubinate_app PASSWORD '$APP_PASSWORD';
    END IF;
  END
  \$\$;

  -- The role needs to CONNECT to the database before it can do
  -- anything else. The migration grants schema-level access; this
  -- handles database-level.
  GRANT CONNECT ON DATABASE "$POSTGRES_DB" TO kubinate_app;
EOSQL
