# Add / remove worker nodes on an existing cluster

**Labels**: `area/workflows`, `area/cluster`, `sprint-3`
**Epic**: Phase 2 finisher
**Size**: 5

## Context

Sprint 1 ticket 03 ships the initial provision; Sprint 1 ticket 05
ships destroy. This ticket adds the in-between case: a Ready cluster
where the user wants to add a worker (load grew) or remove one (cost
optimisation). Both reuse the existing activities — only the workflow
shape is new.

## Acceptance criteria

- **Given** a Ready cluster with N workers, **when** the user POSTs
  `/v1/clusters/:id/workers` with `{ "delta": +1 }`, **then** a
  `ScaleOutWorkflow` runs that creates one Hetzner server, joins it
  via `ssh_install_k3s_agent`, and inserts a `cluster_servers` row.
  `kubectl get nodes` shows the new node within 5 minutes.
- **Given** the same cluster, **when** they POST with
  `{ "delta": -1 }`, **then** a `ScaleInWorkflow` runs that drains
  the chosen worker, deletes the Hetzner server, soft-deletes the
  `cluster_servers` row.
- **Given** `delta` would take the worker count below 1 or above
  the catalog's max (currently 10), **then** the API returns 400
  with a clear message — never starts a workflow.

## Implementation notes

- Reuse `worker_params(index)` from `crates/workflows/src/activities.rs`;
  pick the next free index by counting current `cluster_servers` rows
  with `role = worker`.
- Drain step uses `kubectl drain` + `kubectl delete node` via a new
  `KubectlExecutor` trait that mirrors `HelmExecutor`'s shape.
- Re-uses the `clusters.status` enum: `ready → scaling → ready` for
  scale-out, same for scale-in.

## DoD

- [x] Workflow happy-path tests for both directions with mock
      Hetzner + SSH. (`scale_out_appends_workers_at_the_next_index`,
      `scale_in_drains_then_deletes_each_target` in
      `crates/workflows/src/workflows.rs`.)
- [x] Idempotency: a re-POST with `delta: 0` is a no-op. (API
      handler `scale_workers` returns `200 OK` with the current view
      and never spawns a workflow when `delta == 0`. Re-running a
      partial scale-in is also covered by
      `scale_in_is_idempotent_when_targets_already_gone`.)
- [x] `cluster_servers` row count matches Hetzner reality after
      both flows. (Scale-out: runner calls `record_server` for every
      successful Hetzner create. Scale-in: runner soft-deletes the
      row after `hetzner_delete_server_idempotent` succeeds. Both
      paths run inside the existing tenant-scoped tx so RLS still
      governs.)
- [x] Runbook update: `provisioning-workflow-stuck.md` covers the
      scaling case. (New "Scaling-specific notes" section + two
      kubectl-drain rows in the diagnosis table.)
