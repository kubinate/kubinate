# Author the dogfood-migration spike doc

**Labels**: `area/platform`, `area/sre`, `sprint-4`
**Epic**: Phase 3 thesis — open the dogfood-cluster decision
**Size**: 3 (small ticket, large stakes — the doc itself
authors a Sprint 5+ commitment plan)

## Context

Phase 3 opens with the dogfood migration: running the Kubinate
control plane on our own k3s cluster, the way our customers'
clusters do. The roadmap names "Dogfood cluster running the
control plane" as the Phase 3 exit gate that everything else in
the Sprint 4 parking lot eventually rides on (Vault, agent
tunnel, production observability, threat-model v2 all want a
real production-shape host to land against).

We are not running the migration in Sprint 4. The migration
itself is a Sprint 5+ commitment that this ticket sizes by
producing the **spike-doc starter** that pre-commits the team
to the questions and the recommendation shape. The pattern is
the same as
[`docs/decisions/sprint-2-temporal.md`](../../decisions/sprint-2-temporal.md)
and
[`docs/decisions/sprint-3-observability-tsdb.md`](../../decisions/sprint-3-observability-tsdb.md):

- The structure is the value — the questions the spike must
  answer are pre-committed here.
- The empirical sections are marked
  `<<measurement needed>>` and filled in by the spike-runner
  when Sprint 5 actually executes.
- The default recommendation is asserted from prior art so the
  spike doesn't drift into open-ended exploration.

## When this opens

Concurrently with the rest of Sprint 4. **No** prerequisite on
tickets 02/03/05; this is documentation work that uses
artifacts that already exist (Phase 2 deploy story in
`infra/ansible/` + `infra/terraform/`, ADR-0004 deployment
topology, Sprint 2's HA decision shape). It does **not** open a
real spike — it produces the doc that *will* drive the spike.

## Acceptance criteria

- **Given** the spike-doc starter pattern from ADR-0011 +
  ADR-0012, **when** this ticket ships, **then**
  `docs/decisions/sprint-4-dogfood-migration.md` exists with:
  - An "Honest scope note" header that names this as a
    decision-doc starter (not the spike).
  - A "Context" section linking back to ADR-0004 and naming
    the forcing functions (Phase 3 thesis, marketing
    milestone, prerequisite for Vault / agent / production
    observability).
  - **Four `<<measurement needed>>` questions**: M1 cutover
    wallclock, M2 rollback rehearsal, M3 Postgres data-
    residency on k3s + PVC backups, M4 the `kubinate-api`
    runtime story (statefulset vs deployment, secrets
    injection, OTLP collector reachability).
  - A "Recommendation skeleton" table whose rows flip on the
    M1–M4 outcomes.
  - A "What 'Defer to Sprint 6+' looks like in practice"
    fallback section (mirroring ADR-0011's defer-path
    framing).
  - A "Revisit trigger" section listing the conditions that
    re-open the migration plan if Sprint 5 cannot execute.
  - A "References" section linking ADR-0004,
    `infra/ansible/`, `infra/terraform/`, this ticket, and
    the parent
    `docs/backlog/sprint-4/README.md`.
- **Given** the doc is in place, **when** Sprint 5 planning
  opens, **then** the planning session can pick it up directly
  — no further re-derivation of the migration's shape needed.

## Implementation notes

- The doc is prose; there's no code to write. **Cite** existing
  artifacts rather than re-describing them: ADR-0004 already
  describes the single-VPS topology; the dogfood doc only
  needs to describe the *delta*.
- The four M-questions should be specific enough that a
  spike-runner with no Kubinate context can read the doc and
  know what to measure. Numbers > prose.
- The "Recommendation skeleton" matrix should resolve to a
  default rec **even with all M markers unfilled** — the
  default is asserted from prior art, the same way ADR-0011
  asserted "defer Temporal" before the empirical sections were
  filled.
- My default rec for the dogfood doc, until the spike fills in
  the markers: **incremental migration**, API + observability
  proxy first, then Postgres (with CloudNativePG behind it),
  then Temporal (when it's wired — currently the in-process
  runner ships with the API, so it migrates with the API).

## DoD

- [x] `docs/decisions/sprint-4-dogfood-migration.md` exists
      with all sections from the AC list above. Authored
      2026-05-05.
- [x] No `<<measurement needed>>` markers are filled in by
      this ticket — that's the *next* ticket's job (the
      spike itself). Twelve markers across M1–M4 stay
      open.
- [ ] The doc passes a `pr-review-toolkit:comment-analyzer`
      review for accuracy: every cited file path exists,
      every cited ADR is real, every claim about the Phase 2
      shape is verifiable in code. *Run as part of the
      sprint-close PR review, not at draft time.*
- [x] `docs/backlog/sprint-4/README.md` references this doc
      as the Sprint 5 commitment artifact (see "Sprint 5
      commitments inherited from this sprint").
- [x] `docs/backlog/sprint-5/01-dogfood-migration.md` is
      stubbed (one-paragraph placeholder pointing at this
      doc as the source of truth). Sprint 5 README also
      created at `docs/backlog/sprint-5/README.md`.

## What this ticket deliberately does **not** do

- **Run the spike.** That's Sprint 5's commitment. This
  ticket only writes the doc that pre-shapes the work.
- **Fill in the M1–M4 measurements.** Unfilled markers are
  the explicit signal that the spike hasn't been run yet —
  filling them in here would create the same false-precision
  problem the Phase 2 spike-doc starters explicitly call
  out.
- **Decide CloudNativePG vs. self-managed Postgres on k3s.**
  That's the Sprint 5 / 6 spike doc's question — the
  dogfood doc names "Postgres data-residency on k3s" but
  doesn't pre-commit to an operator choice.
- **Address the Temporal SDK question.** ADR-0011 already
  governs that; the migration doc cites it but doesn't
  re-litigate.
