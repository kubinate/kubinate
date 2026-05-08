# Temporal Rust SDK spike + adopt/defer decision

**Labels**: `area/workflows`, `area/decision`, `sprint-2`
**Epic**: Sprint 1 carry-over
**Size**: 5

## Context

ADR-0003 committed to Temporal as the orchestration engine, with the
Rust SDK as the preferred path and the Go SDK as an explicit fallback
if the Rust SDK proves inadequate. Sprint 1 deferred the wiring to a
follow-up; today the workflow runs via `tokio::spawn` inside the
`kubinate-api` process. This is fine for the thin slice but every
Phase 2 capability we expect (replay debugging, signals, child
workflows, durable scheduling) rides on having the SDK in place.

This is a **spike**: the deliverable is a recommendation document and
either a minimal end-to-end Temporal-backed `provision_cluster` *or* a
written justification for why we should defer to the Go fallback for
another sprint.

## Acceptance criteria

- **Given** the spike, **when** it concludes, **then** there is a
  decision document in `docs/decisions/sprint-2-temporal.md` covering:
  - SDK maturity (compile, replay, versioning, signals working as of
    today).
  - Operational shape (how the worker runs locally + in our planned
    single-VPS topology).
  - Migration cost from the current `LocalRunner`.
  - Go-fallback escape hatch if we hit a blocker.
- **Given** the recommendation is "adopt", **then** the workflow
  function from `crates/workflows/src/workflows.rs` runs as a real
  Temporal workflow against the local Temporal server with the
  existing `provision_cluster` test case translated to a replay test.

## Implementation notes

- Branch off `main`; spike code can live in a feature flag if it
  doesn't make the recommendation cut.
- Temporal server is already in `docker-compose.yml`.
- If adopting: introduce a worker binary (`kubinate-worker`) under
  `crates/workflows/` with an entry point that registers activities.

## DoD

- [ ] Decision doc merged.
- [ ] If adopt → the existing `workflow_is_deterministic_under_replay`
      test ports to Temporal's `WorkflowReplayer` and passes.
- [ ] If defer → an explicit revisit-trigger date or condition is
      named in the decision doc.
