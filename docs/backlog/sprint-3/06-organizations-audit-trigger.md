# Audit trigger for `organizations.plan` changes

**Labels**: `area/platform`, `area/billing`, `sprint-3`
**Epic**: Sprint 2 ticket 08 carry-over
**Size**: 2

## Context

Sprint 2 ticket 08 left a DoD row open: "Audit entries for
`plan.changed` and `stripe.customer_created`". Today the
`subscription_events` table durably logs every Stripe webhook, but
the canonical per-tenant audit hash chain (ticket 09) doesn't see
plan changes. This ticket adds a column-aware trigger on
`organizations` that emits a `clusters.updated`-style audit row only
when `plan` or `stripe_customer_id` actually changes.

## Acceptance criteria

- **Given** an `UPDATE organizations SET plan = 'starter' WHERE id …`,
  **then** `audit_log_append_trigger` fires and writes a
  `organizations.updated` audit row that participates in the per-tenant
  hash chain. `audit_log_verify` returns no broken rows after.
- **Given** an `UPDATE organizations SET display_name = …` (no
  billing fields touched), **then** the trigger does not write an
  audit row — we don't want noise from unrelated mutations.
- **Given** the trigger writes a row, **then** the `metadata`
  column carries `{"old_plan": "free", "new_plan": "starter"}`.

## Implementation notes

- New migration: a column-aware trigger that compares `OLD` and
  `NEW` for the two billing fields and short-circuits if neither
  changed. Cannot use the existing `audit_log_append_trigger`
  unchanged because it fires on every UPDATE.
- Reuse the existing `audit_log_hash_input` helper so the chain
  format stays identical.
- `organizations` is the tenant-boundary table itself, so the
  trigger uses the row's own `id` as `organization_id` (not
  `app.current_tenant_id`).

## DoD

- [ ] Integration test: plan change → audit row exists; non-billing
      update → no audit row.
- [ ] `audit_log_verify` runs clean across both flows.
- [ ] Documentation update in `docs/security/threat-model.md` Flow 2.
