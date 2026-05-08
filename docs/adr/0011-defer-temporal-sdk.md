# ADR-0011: Defer the Temporal SDK adoption to Phase 3

- **Status**: Accepted
- **Date**: 2026-05-04
- **Deciders**: Cluster Orchestrator team
- **Tags**: workflows, orchestration, observability
- **Supersedes**: the "Rust SDK now" portion of [ADR-0003](./0003-cluster-provisioning-orchestration.md).

## Context

ADR-0003 committed to Temporal as the cluster provisioning orchestrator
and named the Rust SDK as the preferred integration. Sprint 1 deferred
the wiring to a small in-process runner
([`crates/workflows/src/runner.rs`](../../crates/workflows/src/runner.rs))
because the thin slice didn't yet need durable schedules, child
workflows, or signals — and the Rust SDK was not stable enough to commit
to.

Sprint 2 ticket 03 produced a decision-doc starter at
[`docs/decisions/sprint-2-temporal.md`](../decisions/sprint-2-temporal.md)
that picked the questions the spike must answer (`M1`–`M4`) and
pre-committed the team to a recommendation shape: **adopt** if M1 ships
clean and M3 replays deterministically, otherwise **defer** to Phase 3
when the dogfood k3s cluster is up.

This ADR ratifies the **defer** path. The empirical sections of the
decision doc remain `<<measurement needed>>`; we are not deciding "the
Rust SDK does not work" — we are deciding **"we are not paying the
adoption cost in Phase 2"**, on the grounds that the in-process runner
fits the single-VPS topology (ADR-0004) and is correct + well-tested.

## Decision

We defer Temporal SDK adoption to **Phase 3** and accept the operational
follow-ups below in lieu of the larger migration.

Concretely, in Sprint 3 (and Phase 2 generally) we will continue to:

- run cluster lifecycle workflows in the in-process
  `LocalRunner` (`crates/workflows/src/runner.rs`);
- mirror progress through the existing `provisioning_workflows` shadow
  table for the dashboard, and through the per-cluster SSE event hub
  (Sprint 3 ticket 10) for live updates;
- treat the Sprint 1 determinism test
  (`workflow_is_deterministic_under_replay`) as our stand-in for
  Temporal's `WorkflowReplayer` until the real one is wired.

We will **not**:

- introduce `temporal-sdk` as a workspace dependency in Phase 2;
- run a Temporal worker process on the single-VPS deploy;
- change the workflow function shapes — they remain plain `async fn`s
  in `crates/workflows/src/workflows.rs`.

## Operational follow-ups (defer-path AC)

These ship as part of this ADR / Sprint 3 ticket 01:

1. **In-flight gauge** — `kubinate_runner_workflows_inflight{kind}` is
   exported on `/metrics`. The `kind` label distinguishes
   `provision`, `destroy`, `install_addon`, `scale_in`, `scale_out`.
   Implementation: an RAII `WorkflowInflightGuard` wrapped around each
   `tokio::spawn` in the runner's `spawn_*` helpers (see
   `crates/workflows/src/runner.rs`).
2. **Process metric infrastructure** — `kubinate_platform::metrics`
   installs the Prometheus recorder once at startup;
   `kubinate-api` serves `/metrics` in text-format `0.0.4`. Future
   gauges + counters land here without further plumbing.
3. **This ADR** — promotes the "What 'Defer' looks like in practice"
   block of the decision-doc starter into a permanent, citable
   record.

## Alternatives considered

### Adopt the Rust SDK now (the M1–M4 happy path)

We rejected this because:

- M1–M4 are still empirical placeholders. Adopting before the spike
  measures means committing to either the SDK or the Go-SDK fallback
  blind, in the middle of Sprint 3.
- The thin slice doesn't yet exercise the features the migration buys
  (durable schedules, signals, queries). The carrying cost of running
  a worker process, learning workflow versioning discipline, and
  growing CI build times is real and would land before the value.
- ADR-0003 explicitly listed the Go SDK as the escape hatch. Re-opening
  that fallback in Sprint 3 would compete with the Phase 2 finishers
  (#08 scale-out, #10 SSE) we have already shipped this sprint.

### Adopt the Go SDK fallback now

Same forcing function as above plus a Go toolchain in CI and a second
container in the deploy. Reserved for the case where M1–M4 measure
"Rust SDK fails replay" — and we will not know that until the spike
runs.

### Run the spike inline this week

We chose to defer the spike alongside the adoption. The spike requires
~3 days of focused work plus a local Temporal server harness; that
budget is better spent in Phase 3 when the dogfood cluster is up and
the operational story (Temporal-on-k3s vs. Temporal-on-VPS) is real.

## Consequences

### We accept

- **No durable workflows yet.** A `kubinate-api` crash mid-provision
  loses the workflow's progress; the runner does not resume on
  restart. The Sprint 1 ticket 05 destroy flow is idempotent, so an
  operator can clean up via re-POSTing destroy.
- **No native signals/queries.** Operator-driven retries of a stuck
  activity are not yet a user-facing feature; the runbook
  (`docs/runbooks/provisioning-workflow-stuck.md`) compensates with
  shell commands.
- **The shadow table is doing real work.** `provisioning_workflows`
  exists because we don't have Temporal history; when we adopt, both
  it and the SSE hub keep their roles (the shadow table is the
  durable record, the hub is the live push).
- **One more ADR to write later.** When Phase 3 runs the spike and
  ratifies "Adopt," that ADR will supersede this one.

### We assume

- **The Rust SDK improves by the time we revisit.** If it
  regresses (or the project loses maintainer momentum), we re-open
  with the Go fallback in scope.
- **The single-VPS runner stays under operational pressure.** If
  `kubinate_runner_workflows_inflight` sustains > 10 across a week,
  that is the trigger to revisit *before* Phase 3.

### Second-order effects

- The metrics infrastructure shipped here is reused by every
  follow-up ADR that wants a Prometheus signal. The cost of the
  next gauge is one line.
- The runner's `spawn_*` helpers are now the canonical entry
  points; the API edge no longer wraps `run_destroy` in a bare
  `tokio::spawn`. Future workflow kinds inherit the gauge for free
  by adding their own `spawn_*` helper.

## Revisit trigger

This ADR re-opens automatically when **any** of the following
observables fire:

1. The `kubinate_runner_workflows_inflight` gauge sustains a value
   greater than 10 (sum across all `kind` labels) over a 1-week
   rolling window. Single-VPS pressure of this shape means we are
   trading correctness against the Temporal investment.
2. The thin-slice production deploy reports a workflow that was
   in-flight when the API crashed and could not be safely re-run by
   an operator (i.e. the lack of durable workflows costs us a real
   incident).
3. Phase 3 begins (week 22+, dogfood-cluster milestone). At that
   point Temporal-on-our-k3s is on the marketing roadmap regardless
   of operational pressure, and we re-spike against the latest
   stable Rust SDK release.

If a trigger fires, we open a follow-up ADR proposing **Adopt** (or
"Adopt with feature freeze" / "Fall back to Go SDK," depending on the
spike result). We do not quietly drift.

## References

- ADR-0003: Cluster provisioning orchestration (the original commit).
- ADR-0004: Deployment topology (single-VPS now, k3s in Phase 3).
- [`docs/decisions/sprint-2-temporal.md`](../decisions/sprint-2-temporal.md): the spike doc starter.
- [`crates/workflows/src/runner.rs`](../../crates/workflows/src/runner.rs): the in-process runner this ADR keeps in service.
- [`crates/platform/src/metrics.rs`](../../crates/platform/src/metrics.rs): the `metrics` recorder + name constants.
- [`docs/backlog/sprint-3/01-temporal-sdk-adoption.md`](../backlog/sprint-3/01-temporal-sdk-adoption.md): the ticket this ADR closes (defer path).
