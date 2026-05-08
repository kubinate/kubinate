# ADR-0005: Primary database and secondary stores

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: data, storage, postgres

## Context

Kubinate needs durable storage for:

- Tenant-scoped OLTP data (users, orgs, clusters, add-on installs,
  audit events, billing records).
- Short-TTL cache and rate-limit state.
- Encrypted secrets (API tokens, kubeconfigs).
- Long-lived blobs (audit archive, Postgres backups, kubeconfig
  snapshots, Velero backups we resell as an add-on feature).
- Temporal's own persistence.

A small team benefits from a small set of storage technologies — every
additional system is another operational surface.

## Decision

- **PostgreSQL 16+** is the primary OLTP store. One cluster (single
  instance in Phase 0–2, CloudNativePG 3-replica in Phase 3+), with
  multiple logical databases:
  - `kubinate` — application data.
  - `temporal` — Temporal's schema. Same instance, separate DB.
- **Redis 7+** for caching, rate limiting, session lookups, and
  pub/sub for WebSocket fan-out to the dashboard.
- **Hetzner Object Storage** (S3-compatible) for backups, archived audit
  logs, kubeconfig snapshots, and any user-facing add-on backup
  capability we expose.
- **Vault** for secret material (introduced in Phase 3, see ADR-0007).

Postgres extensions we commit to using: `pgcrypto` (envelope encryption
of secrets until Vault lands), `uuid-ossp` (not strictly required if we
use UUID v7 generated in the application, but kept for interop),
`pg_stat_statements` (observability).

## Alternatives considered

- **MySQL / MariaDB**: rejected. No strong reason beyond team familiarity
  and the relative weakness of MySQL's row-level security story.
- **Separate OLTP and analytics stores from day one** (e.g. ClickHouse
  for metrics/audit): rejected as premature. Postgres handles our
  expected audit and event volume comfortably at our target scale.
- **Managed Postgres from a third party**: considered for the short term
  but rejected for cost and data-residency consistency (see ADR-0004).
- **Separate Postgres instance for Temporal**: considered. Rejected in
  the short term (operational cost), revisited at the Phase 3
  migration to keep blast radius small.

## Consequences

**We accept:**

- Postgres is a load-bearing component. Backups, PITR, and restore
  drills must be taken seriously from week one; we do not wait for a
  customer incident to learn our recovery procedure.
- Row-Level Security (see ADR-0006) adds complexity to the development
  loop: developers must be disciplined about setting
  `app.current_tenant_id` in each request transaction.
- Redis is another thing to back up (we do not, intentionally — it is
  treated as ephemeral) and another thing to monitor.

**We assume (bets):**

- Postgres scales to our workload for the first 3 years without needing
  sharding or multi-writer topologies. This is supported by publicly
  documented workloads of much larger systems on single-writer Postgres.
- The extension ecosystem we depend on remains compatible across minor
  and major Postgres versions (we will test upgrade paths in staging).

**Positive follow-ons:**

- JSONB columns let us evolve tenant-scoped metadata schemas without
  migrations for every small shape change (for domain-owned "settings"
  blobs, with validation at the application layer).
- `pg_stat_statements` gives us slow-query visibility from day one.
- Logical replication opens the door to read replicas for the
  observability proxy read path later, without re-architecting storage.

## Revisit trigger

- Write QPS on the primary Postgres exceeds 50% of the instance's
  sustained capacity for two consecutive weeks, **or**
- A specific access pattern (e.g. audit log ingestion, metric storage)
  demonstrably does not fit Postgres and is gated on a dedicated store,
  **or**
- Backup / restore drills repeatedly exceed the RTO we have committed to
  in the SLA (Phase 4).

## References

- [PostgreSQL documentation](https://www.postgresql.org/docs/)
- [CloudNativePG](https://cloudnative-pg.io)
- [PostgreSQL Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html)
- [sqlx](https://github.com/launchbadge/sqlx) (Rust Postgres driver).
