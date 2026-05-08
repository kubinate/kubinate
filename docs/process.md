# Engineering Process

The canonical source is project brief §8. This document is the
day-to-day reference.

## Sprint cadence

- **Length**: 2 weeks.
- **Planning**: Monday week 1, 60 minutes.
- **Grooming**: Wednesday week 1, 30 minutes.
- **Review + retrospective**: Friday week 2, 45 minutes combined.
- **Standups**: async, written, in the team channel, time-boxed to
  the contributor's own schedule.

## Backlog hierarchy

- **Epic** — quarter-scale, tracked in GitHub Projects.
- **Story** — sprint-scale, ≤ 5 story points, 1 sprint max.
- **Task** — sub-issue, ≤ 1 day.
- **Spike** — time-boxed investigation, max 3 days.

Sizing uses Fibonacci (1, 2, 3, 5, 8). Anything ≥ 13 gets split.

## Definition of Ready

A story is ready to enter a sprint only when:

1. Acceptance criteria written in Given/When/Then form.
2. UX mock attached, if the story is UI.
3. Architectural impact assessed; ADR referenced or created.
4. Dependencies identified.
5. Sized by the team in planning poker.

## Definition of Done

A story is done when:

1. All acceptance criteria verified.
2. Unit tests added for new logic (coverage targets: 70% on domain
   crates, 50% overall).
3. Integration test for at least the happy path.
4. ADR written if an architectural decision was made.
5. User-facing docs updated.
6. Runbook updated if there is a new operational concern.
7. Security review for auth/permission changes (second-engineer
   approval).
8. PR merged with green CI and at least one other engineer's approval.
9. Feature flag gate in place for anything shippable but not public.
10. Observability: metric and log for new code paths.

## Tech debt quota

**20% of every sprint** is reserved for refactors, dependency upgrades,
flaky-test fixes, and runbook authoring. Tracked as a "debt" epic; new
items are logged to it continuously. Not negotiable when feature
pressure rises.

## Incident management

Incident severities (full rubric in `docs/sre/incidents.md`, TBD):

- **SEV-1**: platform down for a majority of users or data loss risk.
- **SEV-2**: customer-visible degradation; workaround exists.
- **SEV-3**: single-tenant degradation; no customer impact at scale.
- **SEV-4**: internal, no customer impact.

SEV-1 and SEV-2 get a blameless postmortem within 5 business days,
published internally, with action items logged and owned. Quarterly
incident-pattern review to catch systemic issues.

## Code review

- At least one approval required.
- Two approvals required for changes touching `identity`, `authz`,
  `secrets`, or any migration file.
- Target turnaround: < 24h during business hours.

## Commits

Follow [Conventional Commits](https://www.conventionalcommits.org/).
Signed commits are encouraged now; required before GA.
