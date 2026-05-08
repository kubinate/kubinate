# Revisit HA control-plane delivery

**Labels**: `area/workflows`, `area/cluster`, `area/frontend`, `sprint-4`
**Epic**: ADR-0012 revisit
**Size**: 8–13 (depends on which revisit trigger fires; see below)

## Context

[ADR-0012](../../adr/0012-defer-ha-control-plane.md) deferred HA
control-plane delivery to Phase 4+ during Sprint 3. This ticket is
the pre-sized revisit slot so a future operator can pick it up the
moment a revisit trigger from that ADR fires, without re-deriving
the scope from scratch.

The Sprint 2 spike doc
([`docs/decisions/sprint-2-ha-control-plane.md`](../../decisions/sprint-2-ha-control-plane.md))
remains the authoritative source for the empirical sections
(`<<measurement needed>>` placeholders M1–M4) and the recommendation
matrix; this ticket only **delivers** against whichever path the
ratified spike picks.

## When this opens

ADR-0012 names four revisit triggers; this ticket is the work that
follows once any of them fires. **Do not start this ticket
speculatively** — the operational forcing function is what makes
the scope real. The four:

1. A customer with a hard data-residency requirement that forces
   multi-region.
2. Hetzner ships a managed etcd / Cluster API offering.
3. Phase 3 begins (the dogfood-cluster milestone).
4. Sustained sev-2/sev-1 incidents traced to single-CP failure
   under alpha-tenant load.

The first thing to do when this ticket opens is **decide which
trigger fired** — that decides which AC + DoD branch below
applies.

## Acceptance criteria — Same-region 3-CP path (default if M1+M3 PASS)

- **Given** an org with `feature_flags.ha_control_plane = true`,
  **when** they POST `control_plane_count: 3`, **then**
  `cluster::service::validate_allowed` accepts the value (loosens
  the ADR-0012 gate behind the flag, not for everyone).
- **Given** the new `provision_cluster` shape, **when** it runs
  with `control_plane_count: 3`, **then** three CP servers are
  created in the same Hetzner location, joined via
  `k3s server --cluster-init` (first CP) + `k3s server --server …`
  (CPs 2/3), and recorded with `role = control_plane`.
- **Given** a 3-CP cluster, **when** one CP is force-stopped,
  **then** the cluster stays writeable; on restart the workflow's
  reconciliation step rejoins it.

## Acceptance criteria — Cross-region path (if M2 PASS)

Same as above plus:

- **Given** the org has `feature_flags.ha_cross_region = true`,
  **when** they POST a multi-region cluster, **then** the workflow
  spreads CPs across `nbg1` + `fsn1` and the kubeconfig points at
  the first-region CP (load-balancer is a follow-on).

## Acceptance criteria — Managed-etcd path (if Hetzner ships it)

Different shape:

- The k3s install activity uses external etcd (Hetzner-managed)
  rather than embedded. The workflow no longer pays the join
  penalty; the workflow change is smaller but the integration
  testing is heavier.

## Implementation notes

- Migration: `feature_flags JSONB NOT NULL DEFAULT '{}'` on
  `organizations` (this is the gating mechanism the ADR-0012
  defer path explicitly skipped).
- Workflow changes localized to
  `crates/workflows/src/workflows.rs::provision_cluster` —
  Sprint 1's destroy workflow already handles N-of-each-role
  (verified in `destroy_deletes_every_server_in_input`).
- The frontend cluster-create form gets a control-plane-count
  picker that conditionally renders `[1, 3]` based on the org's
  feature flag.
- `validate_allowed`'s rejection path stays for orgs without the
  flag — only loosens for the flagged subset.

## DoD

- [ ] Workflow unit test: 3-CP happy path with mock Hetzner +
      SSH (mirrors `happy_path_runs_all_steps_in_order`).
- [ ] Integration test against a real Hetzner project (CI-gated,
      mirrors `nightly-e2e.yml`) reaches Ready and survives a CP
      power-off.
- [ ] Runbook: `docs/runbooks/etcd-member-unhealthy.md`.
- [ ] Frontend Vitest: feature-flag off → only "1" available;
      feature-flag on → "1" and "3".
- [ ] ADR-0012 superseded by a new ADR that ratifies the chosen
      path. The new ADR cites which revisit trigger fired.
- [ ] No regression on the existing single-CP path: a cluster
      created without the feature flag still goes through the
      same workflow shape it does today.

## What this ticket deliberately does **not** do

- Rolling upgrade of an existing 1-CP cluster to 3-CP. Live
  migration of existing tenants is a separate forcing function;
  it gets its own ticket once a customer asks.
- Add a load-balancer in front of the kube-apiservers. That
  rides with the dogfood-cluster migration (Phase 3 work) — the
  HA path uses the first-region CP's kubeconfig until then.
