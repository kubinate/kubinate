#!/usr/bin/env bash
# Create multiple databases from POSTGRES_MULTIPLE_DATABASES on first-run init.
set -euo pipefail

if [ -z "${POSTGRES_MULTIPLE_DATABASES:-}" ]; then
  exit 0
fi

IFS=',' read -ra DBS <<<"$POSTGRES_MULTIPLE_DATABASES"
for db in "${DBS[@]}"; do
  db="$(echo "$db" | tr -d '[:space:]')"
  if [ "$db" = "$POSTGRES_DB" ]; then
    continue  # already created by the base image
  fi
  echo "Creating database: $db"
  psql -v ON_ERROR_STOP=1 --username "$POSTGRES_USER" <<-EOSQL
    CREATE DATABASE "$db";
    GRANT ALL PRIVILEGES ON DATABASE "$db" TO "$POSTGRES_USER";
EOSQL
done
