# ADR-0010: Authorization

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: security, authorization, rbac

## Context

Once authenticated (ADR-0009), a caller has an identity. Authorization
is the separate question of "is this caller allowed to do this thing to
this resource". For Kubinate, resources are scoped to organizations and,
within an organization, to clusters, add-ons, and settings. Roles and
permissions must be predictable enough for users to reason about
without docs, flexible enough to accommodate small-team usage, and
auditable.

We also need a clear answer to the question of "where does the
authorization check live" — in the handler, in the service layer, in
the database, or all of the above.

## Decision

**RBAC with a fixed role set in v1.** Role-based authorization covers
>95% of real usage; attribute- or policy-based models are deferred
until demand forces them.

Roles (scope: organization):

| Role       | Description                                                    |
|------------|----------------------------------------------------------------|
| Owner      | Full control, billing, destroy organization. 1+ per org.       |
| Admin      | All resource operations except billing and org destruction.    |
| Developer  | Create/modify clusters and add-ons; cannot invite members.     |
| Viewer     | Read-only across resources, including logs and metrics.        |

Per-cluster overrides are **not supported in v1**. A member has a
single organization role. We will revisit if customer demand emerges.

**Implementation layers (defense in depth):**

1. **Application-layer enforcement** (primary): a dedicated `authz`
   module in the `identity` crate exposes `require_permission(actor,
   action, resource)`. Actions are enum values
   (`ClusterCreate`, `ClusterDestroy`, `AddonInstall`, `BillingRead`,
   etc.). Handlers receive an extracted `Actor` from the
   authentication middleware and must call `require_permission` before
   any side-effect. Failure returns HTTP 403 with Problem Details.
2. **Row-Level Security** (ADR-0006) as defense in depth: even if a
   handler forgets the check, the database refuses cross-tenant
   access. This does **not** cover role-based checks within the tenant
   (a Viewer calling a write API would be caught at layer 1, not by
   RLS).
3. **Audit log**: every permission check resulting in a write action
   emits an event with `(actor, action, resource, decision, reason)`.
   Checks that *deny* are also logged; repeated denies are a signal
   worth alerting on.

**Resource identification**: `resource` is a typed URN-like struct
(e.g. `Resource::Cluster { org_id, cluster_id }`). Permission tables
are derived at compile time from an enum match, so adding a new action
requires updating the match (enforced by `#[non_exhaustive]` + the
Rust compiler).

**Machine clients**: service account tokens (ADR-0009) carry an
explicit list of granted actions and optional resource filters. The
`authz` module treats them the same as user actors for the check —
only the source of the permission set differs.

## Alternatives considered

- **ABAC (attribute-based) from day one**: rejected. Over-engineered
  for our scale; the team cost of maintaining policy sets outweighs
  the flexibility benefit.
- **Cedar / OpenFGA / SpiceDB**: all serious tools we are keeping on
  the shelf for Phase 4+. We will adopt one if/when we have concrete
  enterprise requirements (e.g. fine-grained "this user can manage
  this specific cluster but not other clusters in the same org").
- **Database-enforced authorization via Postgres roles per user**:
  rejected — cardinality is wrong and role-switching per request adds
  overhead and complexity.
- **External policy service (OPA sidecar)**: rejected in v1. Network
  hops on the hot path, added infra, solving a problem we do not have
  yet.

## Consequences

**We accept:**

- The fixed role set will feel coarse to some customers. We have a
  clear message: "if you need finer-grained roles, tell us what and
  why — we will track demand and move when we have a real use case".
- Adding a new action type requires code changes (no dynamic role
  definitions). We consider this a feature — it prevents accidental
  permission sprawl.
- Audit logs for denied requests will include low-signal noise (bots,
  scanners). We mitigate with rate-limit-based filtering in the audit
  UI rather than at write time.

**We assume (bets):**

- v1 RBAC covers 95%+ of our real customer needs for the first 12
  months. We validate by tracking "I wish I could express X" signals
  in support and sales.
- The `Resource` enum remains manageable as the domain grows. If it
  balloons, that is itself a signal to move to a resource registry
  pattern.

**Positive follow-ons:**

- `require_permission` is easy to unit-test exhaustively (role × action
  × expected decision matrix).
- The same enforcement module serves the REST API, gRPC API, and
  Temporal activity entrypoints — one place to reason about
  authorization.

## Revisit trigger

- A customer blocks on per-resource role assignments or custom roles
  during sales, **or**
- The number of actions exceeds ~50, at which point maintaining the
  role × action matrix by hand becomes error-prone and we adopt a
  policy engine (likely Cedar), **or**
- An audit finding demonstrates a permission gap in v1 RBAC that
  cannot be patched within the existing role set.

## References

- [NIST RBAC](https://csrc.nist.gov/projects/role-based-access-control)
- [Cedar policy language](https://www.cedarpolicy.com/)
- [OpenFGA](https://openfga.dev/)
- Internal module: `crates/identity/src/authz.rs` (seed).
