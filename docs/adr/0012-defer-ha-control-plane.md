# ADR-0012: Defer HA control-plane delivery to Phase 4+

- **Status**: Accepted
- **Date**: 2026-05-04
- **Deciders**: Cluster Orchestrator team
- **Tags**: cluster, workflows, topology
- **Supersedes**: the implicit "HA lands in Phase 2" note attached to
  the single-CP invariant in
  [`crates/cluster/src/service.rs::validate_allowed`](../../crates/cluster/src/service.rs)
  (it had been a Sprint-1 placeholder; this ADR makes the deferral
  intentional). Does **not** supersede [ADR-0003](./0003-cluster-provisioning-orchestration.md)
  — that ADR's "HA etcd latency on Hetzner" question remains open
  and is the forcing function for the revisit triggers below.

## Context

ADR-0003 §Context flagged "k3s HA etcd latency across Hetzner regions"
as an open question that Sprint 2 would spike. Sprint 2 ticket 10
produced
[`docs/decisions/sprint-2-ha-control-plane.md`](../decisions/sprint-2-ha-control-plane.md),
a decision-doc starter whose default recommendation was **Same-region
3-CP, cross-region deferred**. Sprint 3 ticket 02 was scheduled to
ratify that recommendation and ship the implementation.

We are not running the spike in Sprint 3. The reasons:

1. The spike's measurements (M1–M4 in the decision-doc starter)
   require a real Hetzner project. The team has not yet stood up
   the test project that would run the empirical sections, and
   doing so against a paying customer's project is not
   acceptable. Until that project exists, the spike is just paper.
2. The customer signal we care about for HA is "if one node
   hiccups, my cluster doesn't go down." Phase 2's customer base
   is small teams; their cluster lives on a single VPS today
   (ADR-0004) and they have a single point of failure regardless
   of CP count. **Adding 3-CP to a single-VPS deploy doesn't move
   the failure-tolerance needle**; it adds CP-to-CP RTT pressure
   on a host that's already running Postgres + Temporal + the
   API.
3. Phase 3 begins with a dogfood-cluster migration onto our own
   k3s. That migration is **the** moment to revisit HA: the
   topology is changing anyway, the RTT story is different
   (multi-host private network), and we'll be exercising etcd
   under our own load before a paying tenant does.

This ADR makes the deferral explicit so the single-CP invariant
in `validate_allowed` reads as a deliberate decision, not a
Sprint-1 placeholder.

## Decision

We defer HA control-plane delivery to **Phase 4+** and accept the
operational follow-ups below in lieu of the larger migration.

Concretely, in Sprint 3 (and Phase 2 generally) we will continue
to:

- enforce `control_plane_count == 1` at the cluster-create boundary
  via `kubinate_cluster::service::validate_allowed`, with an error
  message that points the caller at this ADR;
- keep the single-CP path in `kubinate_workflows::workflows::provision_cluster`
  as the only k3s install topology;
- treat `cluster_servers`'s `role = control_plane` rows as a
  single-row class for the purposes of every workflow (destroy,
  scale-in, scale-out are unaffected — they already enumerate
  servers by role).

We will **not**:

- introduce a `feature_flags` JSONB column on `organizations` (the
  3-CP path's gating mechanism);
- ship `cp_params(index)` / k3sup-cluster-init activity changes;
- expose a control-plane-count picker in the frontend
  cluster-create form;
- write the `etcd-member-unhealthy.md` runbook (it's only
  meaningful when there is more than one member to be unhealthy).

## Operational follow-ups (defer-path AC)

These ship as part of this ADR / Sprint 3 ticket 02:

1. **Tightened error message** — `validate_allowed` now points at
   ADR-0012 and names the revisit triggers in the rejection text.
   Test:
   `kubinate_cluster::service::tests::rejects_multi_control_plane_with_adr_pointer`.
2. **Sprint 4 revisit row** — `docs/backlog/sprint-4/01-ha-control-plane-revisit.md`
   pre-sizes the work that lands when a revisit trigger fires.
3. **This ADR** — promotes the spike's deferral stance into a
   permanent, citable record.

## Alternatives considered

### Run the spike inline this sprint and ship Same-region 3-CP

Rejected because:

- The spike requires a Hetzner test project the team hasn't yet
  provisioned. Standing one up + running M1–M4 + writing the
  workflow + writing the runbook + shipping the frontend picker
  is closer to a sprint of work, not the 8 points the backlog
  estimated for the "ratify and ship" path.
- Phase 2 customers don't get failure-tolerance from CP-only HA
  while the deploy itself is single-VPS. The investment is
  premature against the actual customer signal.

### Single-CP-only forever

Rejected. The decision-doc starter named two concrete revisit
triggers (data-residency customer, Hetzner managed etcd) — both
are observable, both are plausible inside our Phase 3 / 4 horizon.
"Forever" closes the door without justification.

### Ship the 3-CP path behind a feature flag without the spike

Tempting because the workflow shape (`cp_params(index)`) is small,
but rejected: a feature-flagged 3-CP path that hasn't been load-
tested against Hetzner private-network etcd is worse than no path
at all. The first paying alpha tenant to flip the flag would be
the spike, and the failure mode (split-brain etcd) is not safe to
discover in production.

## Consequences

### We accept

- **No HA in Phase 2.** A `kubinate-api` host failure (or a CP
  server failure) takes the cluster down. Customers needing
  better than this are told so explicitly during onboarding.
- **No `feature_flags` column.** The shape was useful purely as
  the 3-CP gating mechanism; without that, it's premature
  abstraction.
- **No `etcd-member-unhealthy.md` runbook.** The closest
  operational signal in Phase 2 is the existing
  `provisioning-workflow-stuck.md`, which already covers single-
  CP failures.
- **The error message references this ADR.** Future contributors
  who wonder why CP count is locked to 1 are pointed here in 30
  seconds.

### We assume

- **Phase 3's dogfood cluster reshapes the topology.** When the
  control plane moves off the single VPS onto our own k3s,
  HA-of-the-control-plane and HA-of-the-customer-clusters become
  one decision. This ADR re-opens automatically at that
  milestone.
- **The Hetzner ecosystem stays where it is.** If Hetzner ships
  managed etcd / Cluster API, the cost calculus shifts; we
  re-spike against it (one of the named revisit triggers).

### Second-order effects

- The single-CP invariant in `validate_allowed` is now load-
  bearing rather than a placeholder. Any future change that
  removes it must cite a superseding ADR; otherwise the
  reviewer's `pr-review-toolkit:code-reviewer` agent will flag it.
- The `cluster_servers` table already supports N rows per role
  — that didn't change. When HA does come back, the storage
  shape is unchanged; only the workflow grows.
- The audit chain (ADR-0006) and the SSE event hub (ADR-0011's
  metrics-on-`/metrics`-route work) are agnostic to CP count;
  no follow-up needed in those areas.

## Revisit trigger

This ADR re-opens automatically when **any** of the following
observables fire:

1. **A customer with a hard data-residency requirement** that
   forces multi-region (e.g. EU-only data with fail-over to a
   second EU region). Single-region cannot satisfy this; HA in
   Hetzner means crossing `nbg1` ↔ `fsn1` (or similar), which is
   exactly the cross-region case the M2 measurement targets.
2. **Hetzner publishes a managed etcd / Cluster API offering**
   that removes the embedded-etcd risk. Re-spike against it; the
   workflow change shrinks substantially when we're not managing
   etcd ourselves.
3. **Phase 3 begins** (week 22+, dogfood-cluster milestone). At
   that point HA-of-the-control-plane is one decision with
   "where does Kubinate's own control plane live," and re-
   spiking is forced regardless of customer pressure.
4. **Sustained sev-2/sev-1 incidents traced to single-CP
   failure.** If an alpha tenant hits a CP outage that 3-CP
   would have absorbed, the decision-doc evidence to flip back
   to "ship same-region 3-CP" is on the table.

If a trigger fires, we open a follow-up ADR proposing
**Same-region 3-CP** (or a richer topology, depending on which
trigger fired). We do not quietly drift.

## References

- ADR-0003: Cluster provisioning orchestration (the original commit; HA-etcd-latency open question lives there).
- ADR-0004: Deployment topology (single-VPS now, k3s in Phase 3).
- ADR-0011: Defer the Temporal SDK adoption (sister-doc style for this ADR; same `<<measurement needed>>` discipline).
- [`docs/decisions/sprint-2-ha-control-plane.md`](../decisions/sprint-2-ha-control-plane.md): the spike-doc starter this ADR closes.
- [`crates/cluster/src/service.rs`](../../crates/cluster/src/service.rs): the `validate_allowed` gate this ADR keeps in service.
- [`docs/backlog/sprint-3/02-ha-control-plane.md`](../backlog/sprint-3/02-ha-control-plane.md): the ticket this ADR closes (Single-CP-only path).
- [`docs/backlog/sprint-4/01-ha-control-plane-revisit.md`](../backlog/sprint-4/01-ha-control-plane-revisit.md): the pre-sized revisit ticket.
