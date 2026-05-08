# Sprint 2 spike — Temporal Rust SDK adoption

**Status**: Proposed (recommendation **Defer**, conditional on the
measurements below).
**Decided**: <<2026-04-28; ratify after the spike runs>>
**Spike owner**: Cluster Orchestrator team.
**Supersedes**: portion of ADR-0003 that committed to "Rust SDK now".
**Relates to**: ADR-0003 (cluster provisioning orchestration),
`crates/workflows/src/runner.rs` (the in-process replacement we ship today).

---

## ⚠️ Honest scope note

This file is a **decision-doc starter**, not the spike itself. The
empirical sections below are marked `<<measurement needed>>` — the
spike-runner fills those in by actually building / running. Do not
ratify the recommendation until those markers are resolved.

The structure here is the value: it picks the questions the spike
must answer, lists the tradeoffs that I can already assert from
prior art, and pre-commits the team to a shape for the
recommendation so the spike doesn't drift into open-ended
exploration.

---

## Context

ADR-0003 committed to Temporal as the orchestration engine, with
the Rust SDK as the preferred path and the Go SDK as an explicit
fallback. Sprint 1 deferred the wiring to `tokio::spawn` inside
the API binary
([`crates/workflows/src/runner.rs`](../../crates/workflows/src/runner.rs)).
The current runner is **fine for the thin slice** (Phase 0–1) and
ships the determinism-replay test as a stand-in for Temporal's
`WorkflowReplayer`. But every Phase 2 capability we expect — durable
schedules, child workflows, signals, retry policies with operator
visibility — rides on having a real Temporal SDK in place.

The decision: do we adopt the Rust SDK now, or defer to the Go SDK
fallback (or wait one more sprint for the Rust SDK)?

## Spike scope

Three days of focused work, time-boxed. Deliverable: a fork of
`crates/workflows/` where:

1. The five activities and the `provision_cluster` workflow are
   wired through `temporal-sdk-rust`.
2. `workflow_is_deterministic_under_replay` (the Sprint 1 test in
   `crates/workflows/src/workflows.rs`) ports to Temporal's
   `WorkflowReplayer` and passes against a recorded history.
3. The local Temporal server from `docker-compose.yml` runs the
   workflow end-to-end with a mock Hetzner provider.

Do **not** spike: SSH activity migration (no behaviour change),
production deploy (Sprint 3+), Go-SDK fork (only revisit if (1)
fails).

## What I can assert from prior art

These items don't need empirical measurement — they're stable
facts about the ecosystem we're entering.

### Operational shape (independent of SDK choice)

- The Temporal server we already run in `docker-compose.yml` is the
  reference HA shape: a server (history + matching + frontend) plus
  worker processes. Sprint 2's single-VPS topology (ADR-0004) puts
  all of these on one host; Phase 3 splits them across the dogfood
  k3s cluster.
- Worker-side scaling is the only knob most teams touch. We control
  worker concurrency via `--max-concurrent-activities`; the runbook
  in `docs/runbooks/hetzner-5xx-surge.md` already references this
  even though the runner doesn't enforce it yet.
- Workflow versioning is **its own discipline**. If we adopt now,
  the team writes a one-page guide on `temporal::patched` /
  `temporal::version` before any second feature branch lands.

### Migration cost from `LocalRunner`

The shape of the swap is small; the costs are:

- **Activity registration** — each `pub async fn` in
  `crates/workflows/src/activities.rs` becomes a registered
  activity. Mechanical.
- **Workflow function** — `provision_cluster` runs in a
  workflow-context executor. Side effects (`tokio::time::sleep`)
  must move into activities or use Temporal's deterministic timer.
- **Runner dies** — the `LocalRunner` struct + the
  `provisioning_workflows` shadow table writer become a Temporal
  worker binary (`crates/workflows/bin/worker.rs`). API just
  starts workflows via the Temporal client.
- **Tests** — happy path port to Temporal's `WorkflowReplayer`.
  The mock providers we already have don't need to change.
- **Cargo deps** — new top-level `temporal-sdk` (and its tonic /
  prost dependency tree). Build time goes up; CI cache hits cover it.

### Go-SDK fallback

If Rust SDK fails the spike, the Go fallback is well-understood:

- A new repo / subtree `workers-go/` with the workflow and
  activities re-implemented in Go. Activities call into the Rust
  binary's HTTP API (or a thin gRPC) for shared logic.
- Adds a Go toolchain to CI and a second container in the deploy.
- The Rust API service stays unchanged — it only invokes Temporal
  workflows by name.

## What needs measurement (the actual spike)

### M1 — SDK maturity

- Does `temporal-sdk` compile against our workspace's Rust 1.90
  + edition2021 + the dep tree we ship today (axum 0.8, tokio 1,
  sqlx 0.8)?
  - **Result**: <<measurement needed>>

- Does the recommended hello-world ship a workflow + activity
  + replayer round-trip without panics?
  - **Result**: <<measurement needed>>

- Are signals + queries available, or do they require nightly?
  - **Result**: <<measurement needed>>

### M2 — `provision_cluster` round-trip

Implement against the local Temporal server:

- All five activities registered, mock Hetzner + SSH providers
  injected.
- Workflow runs end-to-end on `cargo run -p kubinate-worker` while
  the API issues a `start_workflow` from a test endpoint.
- Result + execution time:
  - **Wallclock from start to "done" step**: <<measurement needed>>
  - **Workflow history size**: <<measurement needed>>

### M3 — Replay test

Capture history from M2, feed to `WorkflowReplayer`. Does it
replay deterministically?

- **Result**: <<measurement needed>>

### M4 — Operational footprint

Memory + CPU of the worker binary at idle and under one
in-flight workflow:

- **Idle RSS**: <<measurement needed>>
- **In-flight RSS**: <<measurement needed>>
- **CPU during a single provision**: <<measurement needed>>

These set the budget for Phase 3 when we run multiple workers
on the dogfooded k3s.

## Recommendation skeleton

The recommendation flips on M1 and M3:

| M1 maturity | M3 replay | Recommendation |
|---|---|---|
| Compiles + hello-world works | Deterministic replay | **Adopt Rust SDK** in Sprint 3. |
| Compiles but signals/queries broken | n/a | **Adopt Rust SDK with feature freeze** (no signals until upstream). |
| Compile fails or hello-world panics | n/a | **Defer**: extend the in-process runner one more sprint, file an upstream bug, revisit Sprint 4. |
| Compiles but replay non-deterministic | n/a | **Fall back to Go SDK** (per ADR-0003 escape hatch). |

Until M1–M4 are filled in, my **default recommendation is Defer**:
the in-process runner is correct, well-tested, and fits the
single-VPS topology. The cost of adopting SDK + worker binary +
versioning discipline is real; we should not pay it before the thin
slice has hit production.

## What "Defer" looks like in practice

If the team accepts Defer:

- Add `KubinateRunnerWorkflowsInflight` Prometheus metric (Sprint 3
  ticket): when the in-process runner is processing more than ~10
  concurrent workflows on the single VPS, that's the operational
  pressure we're trading against the Temporal investment.
- Re-spike at the start of Phase 3, *after* the dogfooded k3s
  cluster is up — running Temporal on top of our own platform is
  the marketing milestone the brief lays out.

## What "Adopt Rust SDK" looks like in practice

Sprint 3 ticket sized after M1–M4 are in. Skeleton:

- 5 pts — port activities + workflow function.
- 3 pts — worker binary + deploy via Ansible (extend
  `infra/ansible/`).
- 3 pts — replay test ported, runbook (`provisioning-workflow-stuck.md`)
  updated with the new diagnostic commands.
- 2 pts — versioning guide + first `temporal::patched` example.

## Revisit trigger

- M1 compile failure that we expect to be fixed upstream within
  one sprint of when it's reported. Revisit on the next stable
  release.
- The single-VPS runner shows operational pressure (the Prometheus
  metric above sustained > 10 concurrent workflows across a week).

## References

- ADR-0003: Cluster provisioning orchestration (the original commit).
- ADR-0004: Deployment topology (single-VPS now, k3s in Phase 3).
- `crates/workflows/src/runner.rs`: today's stand-in.
- `crates/workflows/src/workflows.rs#workflow_is_deterministic_under_replay`:
  the determinism check that ports to `WorkflowReplayer`.
- `https://github.com/temporalio/sdk-core` and the Rust SDK crate
  the spike consumes.
