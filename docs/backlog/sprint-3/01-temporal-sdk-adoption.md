# Adopt the Temporal Rust SDK (or formally defer)

**Labels**: `area/workflows`, `sprint-3`
**Size**: 8 (3 if the spike recommended Defer)
**Epic**: Sprint 2 spike ratification

## Context

Sprint 2 ticket 03 produced
[`docs/decisions/sprint-2-temporal.md`](../../decisions/sprint-2-temporal.md).
This ticket implements the recommendation that came out of M1–M4,
or — if M1 failed — captures the deferral with the operational
follow-ups the decision doc names.

## Acceptance criteria — adopt path

- **Given** the spike recommended "Adopt", **when** this ticket
  ships, **then**:
  - `crates/workflows/src/runner.rs::LocalRunner` is replaced by a
    Temporal worker binary `crates/workflows/bin/worker.rs` that
    registers the existing five activities + `provision_cluster`
    and `destroy_cluster` workflows.
  - The `provisioning_workflows` shadow table writes move from
    `WorkflowProgress` (today's `ProgressSink` impl) to a Temporal
    activity that the workflow calls between steps.
  - `workflow_is_deterministic_under_replay` ports to Temporal's
    `WorkflowReplayer` and passes against a recorded history.
  - `infra/ansible/` adds a `temporal-worker` role that runs the new
    binary as a systemd service alongside the api.

## Acceptance criteria — defer path

- **Given** the spike recommended "Defer", **when** this ticket
  ships, **then**:
  - [x] A `KubinateRunnerWorkflowsInflight` Prometheus gauge exists
        on `kubinate-api` (counts in-flight workflows in the
        in-process runner). Implemented as
        `kubinate_runner_workflows_inflight{kind=…}` via an RAII
        `WorkflowInflightGuard` wrapped around each `spawn_*` helper
        in `crates/workflows/src/runner.rs`. Exposed at
        `GET /metrics`.
  - [x] The decision doc's "What 'Defer' looks like in practice"
        block is moved into a follow-up ADR with `status:
        Accepted`. See
        [`docs/adr/0011-defer-temporal-sdk.md`](../../adr/0011-defer-temporal-sdk.md).
  - [ ] A revisit-trigger calendar entry is set for the date the
        decision doc names. *Operator follow-up*: ADR-0011 names
        three observables (sustained inflight > 10, real-incident
        loss, Phase 3 milestone) — the literal calendar entry is the
        Phase-3-start one (week 22+). File the calendar invite
        manually; the ADR is the authoritative copy of the
        triggers.

## Implementation notes

- Worker binary: keep activity bodies untouched; only the
  registration boilerplate is new.
- Backwards compatibility: the API still calls
  `runner.spawn_provision(...)` style methods; under the hood that
  becomes a Temporal client `start_workflow` call. Handlers don't
  change.

## DoD

- [ ] Replay test green against a recorded production-shape history.
- [ ] `infra/ansible/` deploys the worker on the existing VPS with
      no additional manual steps.
- [ ] `docs/runbooks/provisioning-workflow-stuck.md` updated with
      the new diagnostic commands (Temporal CLI vs. raw psql).
- [ ] No regression on the 39 existing unit tests.
