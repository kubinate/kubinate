# Run the dogfood-cluster migration spike (M1–M4)

**Labels**: `area/platform`, `area/sre`, `sprint-5`
**Epic**: Phase 3 thesis
**Size**: 5 (high-confidence; the questions and structure are
already defined by `docs/decisions/sprint-4-dogfood-migration.md`)

## Context

Sprint 4 ticket 06 shipped the spike-doc starter at
[`docs/decisions/sprint-4-dogfood-migration.md`](../../decisions/sprint-4-dogfood-migration.md)
with `<<measurement needed>>` markers across M1–M4. The
recommendation column is conditional: until the markers
resolve, the doc's status is `Proposed` and the rest of
Sprint 5 is sized against the *default* recommendation
(order (A), API-first incremental migration). This ticket
runs the spike and ratifies the doc — the unlock for #02
through #06.

This is **not** the dogfood-cluster bring-up. That's #02.
This ticket runs the migration in a staging environment,
records what happens, and either confirms or invalidates the
default-recommendation row. If the spike invalidates the
recommendation, #02 is re-shaped before the cluster bring-up
window opens.

## Acceptance criteria

- **Given** an isolated staging environment that mirrors the
  Phase 2 single-VPS deployment, **when** the spike-runner
  executes order (A) — API to k3s, Postgres staying on the
  VPS — **then** every `<<measurement needed>>` marker in
  M1 (cutover wallclock) resolves with a concrete value.
- **Given** the same staging environment, **when** the
  spike-runner executes M2 (rollback rehearsal), **then** the
  rollback completes inside the 60-minute pass criterion AND
  zero tenant-visible rows are lost. A failed M2 pass
  criterion stops the spike — re-shape happens before any
  further measurement runs.
- **Given** order (A) and order (B) both run in the staging
  environment, **when** the spike-runner records each
  ordering's M1 wallclock, **then** the doc's recommendation
  row picks based on the < 60s customer-visible-gap rule
  named in the doc.
- **Given** every M-marker is resolved, **when** the spike
  closes, **then** the doc's status header flips from
  `Proposed` to `Accepted` (or a sibling ADR records the
  divergence and the doc gets a `Superseded by ADR-XXXX`
  note).

## Implementation notes

- **Staging environment shape.** Use a fresh Hetzner project
  (separate from the test project the e2e harness runs in,
  separate from production) with one VPS provisioned per the
  Phase 2 Ansible roles, plus a budget for the second
  short-lived 1-CP + 2-worker k3s cluster M1 brings up. The
  spike-runner is responsible for cleanup after each
  ordering; orphan-sweep applies the same way as the e2e
  harness (label every spike resource with
  `kubinate.spike.run_id` for the destroy pass).
- **M1 wallclock instrumentation.** The synthetic monitor
  cluster-create call is the customer-visible-gap proxy. Run
  it on a 5-second cadence during the cutover window; the
  gap = number of consecutive 5xx responses × 5 seconds + the
  partial 5xx at either end. This is the same shape the
  Sprint 1 nightly E2E harness uses; reuse the synthetic
  monitor code if practical.
- **M2 zero-data-loss check.** Snapshot the staging
  Postgres before the cutover (pg_dump). After rollback,
  diff every tenant-scoped table against the snapshot. The
  expected diff is exactly the rows the synthetic monitor
  inserted during the cutover window — anything else is
  data loss and the M2 pass criterion fails.
- **M3 / M4 timing.** M3 (Postgres on k3s viability) and
  M4 (single-CP failover RPO/RTO) run after M1/M2 because
  they want a passing M2 first — running them on a broken
  rollback shape conflates root causes.
- **Cost discipline.** The staging environment runs for the
  duration of the spike (~4 days max). Budget the spike at
  ~€50 of Hetzner spend; if the bill exceeds that, the spike
  pauses and we resize the cluster shape before continuing.

## DoD

- [ ] M1 markers resolved in
      `docs/decisions/sprint-4-dogfood-migration.md` — both
      orderings (A) and (B) measured.
- [ ] M2 markers resolved; the rollback rehearsal either
      passes or the spike halts and re-shapes.
- [ ] M3 markers resolved (Postgres on k3s footprint +
      operator selection — CloudNativePG vs Stolon vs
      single-pod).
- [ ] M4 markers resolved (single-CP failover behavior;
      this informs the Sprint 6 HA-revisit ticket's
      trigger conditions).
- [ ] Doc status flipped to `Accepted` (or a divergent ADR
      written and the doc marked `Superseded`).
- [ ] A 1-page summary post under `docs/decisions/` (not a
      new file — append to the spike doc) records the
      empirical findings beyond what the markers needed:
      anything surprising the spike-runner observed that
      Sprint 5+ needs to know.
- [ ] Sprint 5 plan README's "Standing assumptions" section
      reviewed and updated if the spike invalidated any.

## What this ticket deliberately does **not** do

- **Bring up the production dogfood cluster.** That's #02.
  This ticket runs the spike in a staging environment and
  destroys it.
- **Migrate any production data.** Staging only.
- **Ratify the CloudNativePG operator choice as
  authoritative.** M3 picks an operator for the staging
  spike; the production-dogfood operator choice gets a
  follow-up ADR in Sprint 6 once the dogfood cluster runs
  with real workloads. The Sprint 6 ticket inherits M3's
  recommendation as a starting point, not a commitment.

## Why this is its own ticket and not part of #02

- The spike's **outcome** changes #02's shape. Bundling
  them puts #02 on the critical path before it's been
  validated, which is the failure mode the spike-doc
  convention exists to prevent.
- The spike runs in staging; #02 runs in (test-)production.
  Different environments, different reversibility profiles,
  different security review pressures.
- A failed M2 pass criterion blocks #02 entirely. Naming
  that as a separate gate keeps the failure mode loud.

## References

- [`docs/decisions/sprint-4-dogfood-migration.md`](../../decisions/sprint-4-dogfood-migration.md)
  — the spike doc this ticket runs.
- [`docs/backlog/sprint-4/06-dogfood-migration-spike.md`](../sprint-4/06-dogfood-migration-spike.md)
  — Sprint 4's parent ticket that pre-shaped this work.
- [`docs/backlog/sprint-4/README.md`](../sprint-4/README.md)
  § "Sprint 5 commitments inherited from this sprint".
