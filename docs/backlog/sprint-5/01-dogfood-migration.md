# Run the dogfood-cluster migration

**Labels**: `area/platform`, `area/sre`, `sprint-5`
**Epic**: Phase 3 thesis
**Size**: 26 (placeholder — re-size after the spike fills in
M1–M4; the Sprint 4 README's "Sprint 5 commitments inherited
from this sprint" section pre-loads ~26 pts of work that
splinters into multiple tickets if the team prefers)

## One-paragraph stub

This is the **execution** of the migration plan ratified in
[`docs/decisions/sprint-4-dogfood-migration.md`](../../decisions/sprint-4-dogfood-migration.md).
The decision-doc starter is the source of truth for ordering,
recommendation, and rollback shape; this ticket only ships the
work the spike's recommendation row (M1–M4 outcome) commits to.

Do **not** size or schedule this ticket until the Sprint 4
spike doc has been ratified — every detail (Postgres operator
choice, API runtime shape, big-bang vs incremental, fallback
to "Defer to Sprint 6+") flips on the empirical sections that
Sprint 5 fills in.

## Sprint 5 planning prerequisites

Before this ticket is pickable, the Sprint 5 planning session
must:

- [ ] Run the M1–M4 measurements per the spike doc's "What
      needs measurement" section.
- [ ] Resolve every `<<measurement needed>>` marker in
      `docs/decisions/sprint-4-dogfood-migration.md`.
- [ ] Pick a row from the spike doc's recommendation skeleton
      based on the measurements. Document the choice in the
      same doc's status header (Proposed → Accepted) plus a
      ratifying ADR (next free; likely 0014 if the WebAuthn
      device-lifecycle ADR took 0013 in Sprint 4).
- [ ] **Decide whether to size as one ticket or several.**
      The Sprint 4 README pre-loaded:
      - Run the spike (5 pts) — already covered above.
      - Stand up the dogfood cluster (8 pts).
      - Migrate `kubinate-api` onto it (8 pts).
      - Deploy Vault and run the Sprint 4-deferred Vault
        migration binary (5 pts).
      A 26-point single ticket is too coarse; default split
      is one ticket per row.

## What this stub does **not** do

- **Pre-commit any of the migration's shape.** That's the
  spike doc's job. Splitting the ticket, picking the order,
  picking the operator: all gated on M1–M4.
- **Replace the spike doc.** This stub is the placeholder so
  Sprint 5 planning has a starting line; the doc is where
  every actual decision lives.

See:
[`docs/decisions/sprint-4-dogfood-migration.md`](../../decisions/sprint-4-dogfood-migration.md)
+ [`docs/backlog/sprint-4/06-dogfood-migration-spike.md`](../sprint-4/06-dogfood-migration-spike.md)
+ [`docs/backlog/sprint-4/README.md`](../sprint-4/README.md)
§ "Sprint 5 commitments inherited from this sprint".
