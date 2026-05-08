# Ship 3-node control plane behind a feature flag

**Labels**: `area/workflows`, `area/cluster`, `area/frontend`, `sprint-3`
**Size**: 8 (3 if the spike recommended Single-CP-only)
**Epic**: Sprint 2 spike ratification

## Context

Sprint 2 ticket 10 produced
[`docs/decisions/sprint-2-ha-control-plane.md`](../../decisions/sprint-2-ha-control-plane.md).
This ticket implements the recommended topology behind a feature flag
so we can validate against alpha tenants before defaulting to it.

## Acceptance criteria — Same-region 3-CP path

- **Given** an org with `KUBINATE__FEATURE_HA_CP=1` set in their
  organization metadata (Sprint 3 introduces a `feature_flags` JSONB
  column), **when** they POST a cluster create with
  `control_plane_count: 3`, **then**:
  - `cluster::service::validate_allowed` accepts the value.
  - `provision_cluster` creates 3 CP servers in the same Hetzner
    location and joins them via `k3sup install --cluster --server …`.
  - `cluster_servers` records all three with `role = control_plane`.
  - The cluster reaches `Ready` in < 15 minutes p50.
- **Given** a 3-CP cluster, **when** one CP is force-stopped via the
  Hetzner console, **then** the cluster stays writeable; when the CP
  is restarted, the workflow's reconciliation step rejoins it.

## Acceptance criteria — Single-CP-only path

- **Given** the spike recommended Single-CP, **when** this ticket
  ships, **then**:
  - [x] `validate_allowed` keeps the existing
        `control_plane_count == 1` invariant with a clearer error
        message. The new message points at ADR-0012 and names two
        of the four revisit triggers verbatim, so the rejection
        reads as a deliberate decision rather than a Sprint-1
        placeholder. Test:
        `kubinate_cluster::service::tests::rejects_multi_control_plane_with_adr_pointer`.
  - [x] A new ADR records the decision with the spike measurements.
        See [`docs/adr/0012-defer-ha-control-plane.md`](../../adr/0012-defer-ha-control-plane.md),
        status Accepted. Honest framing: the spike's empirical
        sections are still `<<measurement needed>>` — this ADR
        defers running them, not "the spike said no."
  - [x] Sprint 4 backlog has an HA revisit ticket sized off the
        decision doc's revisit-triggers.
        [`docs/backlog/sprint-4/01-ha-control-plane-revisit.md`](../sprint-4/01-ha-control-plane-revisit.md)
        with three branches (Same-region 3-CP / Cross-region /
        Managed-etcd) keyed off which trigger fires first.

## Implementation notes

- New migration: `feature_flags JSONB NOT NULL DEFAULT '{}'` on
  `organizations`.
- Workflow changes localized to `provision_cluster` — the Sprint 1
  destroy workflow already handles N-of-each-role.
- Frontend cluster-create form: control-plane-count select renders
  `[1, 3]` if the org has the feature flag; otherwise `[1]` only.

## DoD

The DoD rows below are shaped around the **3-CP path** (the
default-rec scope). The Single-CP-only path that this ticket
actually shipped does not produce 3-CP code, so those rows are
**N/A by design**, not "still open." The Single-CP path's own
DoD (the three AC rows above) is what gates this ticket's
closure.

- [N/A] Workflow unit test: 3-CP happy path with mock Hetzner +
        SSH. *Picked back up in Sprint 4 ticket
        `01-ha-control-plane-revisit.md`.*
- [N/A] Integration test against a real Hetzner project (CI
        gated) reaches Ready and survives a CP power-off. *Same.*
- [N/A] Runbook: `docs/runbooks/etcd-member-unhealthy.md`. *Same;
        only meaningful when there is more than one etcd member.*
- [N/A] Frontend Vitest: feature-flag off → only "1" available;
        feature-flag on → "1" and "3". *Same; the feature flag
        column itself was explicitly skipped per ADR-0012.*
