# HA k3s control plane spike on Hetzner private network

**Labels**: `area/workflows`, `area/cluster`, `area/spike`, `sprint-2`
**Size**: 5
**Epic**: Phase 2 starter

## Context

Sprint 1's `provision_cluster` hard-codes `control_plane_count = 1`.
ADR-0003 §Context flagged "k3s HA etcd latency across Hetzner regions"
as an open question to spike in Phase 1, and the project brief schedules
the actual HA delivery for Phase 2. This ticket is the **spike**: a
two-day investigation that produces enough evidence to scope the real
HA work in Sprint 3.

## Acceptance criteria

- **Given** the spike, **when** it concludes, **then** there is a
  decision document in `docs/decisions/sprint-2-ha-control-plane.md`
  covering:
  - Measured etcd write latency on a 3-node embedded-etcd k3s cluster
    in `nbg1`, `fsn1`, and across both (private network span).
  - Recommended topology for HA: same-region 3-CP, cross-region 3-CP,
    or external etcd.
  - Failure-mode behaviour observed: kill one CP and rejoin; how long
    does the cluster stay readable / writeable.
  - Rough provisioning workflow shape (which activities change, which
    are new).
- **Given** the recommendation, **then** Sprint 3's HA delivery
  ticket is sized in story points based on the spike's findings.

## Implementation notes

- Use the existing nightly E2E harness as a starting point — clone
  it into a one-shot `kubinate-ha-spike` binary that provisions 3-CP
  + 1-worker, runs the latency probe, then tears down.
- Stay inside the Hetzner test project; force the spike to share the
  60-minute force-destroy backstop from ticket 08.
- No production code changes from this ticket — the spike's purpose
  is the decision doc.

## DoD

- [ ] Decision doc merged.
- [ ] Spike binary deleted (or moved to `crates/spikes/`) before the
      sprint closes — keep main lean.
- [ ] Sprint 3 HA ticket drafted in `docs/backlog/sprint-3/` with
      explicit scope based on the decision.
