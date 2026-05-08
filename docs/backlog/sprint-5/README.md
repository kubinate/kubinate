# Sprint 5 backlog (stubbed)

Sprint 5 has not been planned. This directory holds the
**stub commitments** Sprint 4 inherited to it via the
dogfood-migration spike doc.

The Sprint 5 planning session opens once Sprint 4 closes — at
that point the spike-doc's `<<measurement needed>>` markers
(`docs/decisions/sprint-4-dogfood-migration.md`) get filled
in, and the recommendation row that fires shapes the actual
Sprint 5 plan.

## Stubbed tickets

| File | Title | Why it's stubbed |
|---|---|---|
| [01-dogfood-migration.md](./01-dogfood-migration.md) | Run the dogfood-cluster migration | Single placeholder for the Phase 3 thesis. Splits into multiple tickets at planning time per the spike doc's recommendation. |

## What Sprint 5 picks up that's already pre-loaded

Per the Sprint 4 README's "Sprint 5 commitments inherited
from this sprint":

- Run the dogfood-migration spike (M1–M4 measurements). ~5 pts.
- Stand up the dogfood cluster (Ansible/Terraform delta + k3s
  deploy). ~8 pts.
- Migrate `kubinate-api` onto the dogfood cluster. ~8 pts.
- Deploy Vault on the dogfood cluster + run the Sprint
  4-deferred Vault migration binary. ~5 pts.

That's ~26 pts pre-loaded; Sprint 5 planning tunes from there.

## What Sprint 5 *might* also need (depends on M-outcomes)

If the spike doc's recommendation row picks the
"in-process runner leaks under restart" branch:

- Re-open ADR-0011 (Temporal SDK adoption) with a follow-up
  ticket sized off whichever recommendation row from
  `docs/decisions/sprint-2-temporal.md` the team picks.

If the rollback rehearsal (M2) fails:

- Re-spike the migration with a load-balancer-aware cutover.
  This is the "Defer to Sprint 6+" path the Sprint 4 spike
  doc names; Sprint 5 ships Vault on the VPS + agent binary
  via cloud-init in the meantime.

## Definitions

- **DoR**: see `/docs/process.md` §4.
- **DoD**: see `/docs/process.md` §5.
