# ADR-0006: Multi-tenancy isolation model

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: data, security, multi-tenancy

## Context

Kubinate is multi-tenant from day one. A cross-tenant data leak is an
existential incident for a B2B infrastructure product. The decision
space for tenancy ranges from "shared database, shared schema,
application-layer filtering" to "database per tenant".

Constraints we care about:

- **Cost**: database-per-tenant is operationally infeasible for a free
  tier and unaffordable for the low-end of our paid tier.
- **Defense in depth**: we want at least two independent layers that
  would have to both fail for a cross-tenant leak to occur.
- **Operational simplicity**: a small team cannot fan out migrations
  across thousands of schemas.
- **Escalation path for enterprise**: some customers will eventually
  demand stronger physical isolation, and we want to accommodate that
  without a ground-up rewrite.

## Decision

**Default tier (Free, Starter, Pro): shared database, shared schema, with
tenant isolation enforced by PostgreSQL Row-Level Security (RLS).**

Concretely:

- Every tenant-scoped table has an `organization_id UUID NOT NULL`
  column.
- Every such table has an `ENABLE ROW LEVEL SECURITY` policy that
  restricts `SELECT`, `INSERT`, `UPDATE`, `DELETE` to rows whose
  `organization_id` equals the session's
  `current_setting('app.current_tenant_id')`.
- Policies are created `FORCE ROW LEVEL SECURITY` so they apply even to
  the table owner.
- The application opens every request-scoped transaction with
  `SET LOCAL app.current_tenant_id = $1` (or sets the variable on a
  connection-pooled session it immediately closes). This is centralized
  in a `TenantScopedTransaction` wrapper in the `platform` crate — no
  domain code issues raw SQL without going through it.
- A separate, privileged DB role (used by Temporal workers running
  cross-tenant system tasks, and by the audit-archive job) explicitly
  `SET LOCAL ROLE` to bypass RLS, and its use is logged.

Defense-in-depth layers:

1. **Application authorization** (ADR-0010): the service layer verifies
   the caller's membership in the target organization before issuing
   any query.
2. **Row-Level Security** in Postgres: even if application code forgets
   a filter or is compromised by a bug, the database refuses to return
   cross-tenant rows.
3. **Integration tests** that assert cross-tenant queries return zero
   rows and that a forged `organization_id` in a request body is
   rejected by both layers.

**Enterprise tier (future, Phase 4): opt-in dedicated database.** Same
codebase, tenant-id-to-DSN mapping resolved at session start. The
application code does not change; only the connection acquisition
strategy does.

## Alternatives considered

- **Shared DB, separate schema per tenant**: rejected. Postgres migration
  fan-out becomes painful quickly (our target is up to a few thousand
  tenants), and the isolation benefit over RLS is marginal.
- **Database per tenant from day one**: rejected for cost and
  operational reasons above.
- **Application-layer filtering only, no RLS**: rejected as violating
  defense in depth. A single SQL query written without the tenant
  filter becomes an incident.
- **Sharded tenants by hash of tenant_id**: rejected as premature.

## Consequences

**We accept:**

- RLS forces every developer to understand transaction scope and
  session GUCs. We mitigate with `TenantScopedTransaction` and by
  refusing to accept DB code in review that bypasses it without an
  explanatory comment linking to the audit-logged exception path.
- Some query patterns (cross-tenant admin operations, reporting, backup
  exports) must explicitly use the bypass role. Each such use is logged.
- Postgres query plans involving RLS policies can surprise us; we plan
  for this by reviewing `EXPLAIN` output on tenant-scoped queries that
  hit large tables.

**We assume (bets):**

- The RLS layer holds up under realistic load. Documented Postgres
  workloads using RLS at meaningful scale exist; we will load-test.
- Our engineers will not normalize "just use the bypass role" as a
  shortcut; we enforce via code review, a small set of approved
  bypass sites, and audit logging.

**Positive follow-ons:**

- RLS gives us a defensible "how do you prevent cross-tenant leaks"
  answer in enterprise sales conversations.
- The same policy framework protects against internal misuse: an
  engineer with production DB access still sees only the tenant they
  have identified themselves as, unless they escalate explicitly.

## Revisit trigger

- A near-miss cross-tenant leak incident where application code bypassed
  the intended path (we validate that RLS caught it, and we keep it —
  but we then also invest in stronger static analysis), **or**
- A customer demands schema-per-tenant or dedicated-DB isolation and the
  escalation path we designed proves insufficient, **or**
- RLS policy overhead materially impacts query latency (P95 degradation
  attributable to RLS > 20% for hot queries).

## References

- [PostgreSQL Row Security Policies](https://www.postgresql.org/docs/current/ddl-rowsecurity.html)
- [Supabase's RLS guide](https://supabase.com/docs/guides/database/postgres/row-level-security)
  (not a dependency, but a clear public reference for patterns).
