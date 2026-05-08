# First add-on workflow: ingress-nginx via Helm

**Labels**: `area/addons`, `area/workflows`, `sprint-2`
**Size**: 5
**Epic**: Phase 2 starter

## Context

The product promise is "baseline add-ons one click away". Sprint 2
ships the very first one — ingress-nginx — to validate the add-on
catalog shape end to end. Cert-manager / Loki / Velero land in
later sprints once we trust the seam.

## Acceptance criteria

- **Given** a Ready cluster, **when** the user POSTs
  `/v1/clusters/{id}/addons` with `{ "addon": "ingress-nginx",
  "version": "4.10.x" }`, **then** an `InstallAddonWorkflow` runs that
  applies the upstream Helm chart via the cluster's kubeconfig and
  records a row in `cluster_addons`.
- **Given** the addon is installed, **when** the cluster status page
  is viewed, **then** ingress-nginx appears in a per-cluster addon
  list with its version + state (`installed | upgrading | failed`).
- **Given** a re-POST with the same `(cluster, addon)` pair, **then**
  the workflow no-ops and returns the existing row.

## Implementation notes

- New crate module: `kubinate-addons::catalog` enumerating the
  allowlist (`ingress-nginx`, `cert-manager`, etc.) with their chart
  repos pinned.
- New activity: `HelmInstallChart(kubeconfig, chart, version, values)`
  shelling out to the `helm` binary (matching the SSH executor's
  subprocess pattern from Sprint 1 ticket 08).
- New table: `cluster_addons` with RLS, audit trigger, status enum.
- Workflow: linear `DownloadChart → HelmInstall → AssertReady`.

## DoD

- [ ] Unit test for the catalog allowlist (rejects unknown addon).
- [ ] Workflow happy-path test with a fake `HelmExecutor`.
- [ ] Integration test (CI gated, manual trigger) installing the chart
      against a kind/k3d cluster — keeps the nightly Hetzner job lean.
- [ ] Runbook stub for "addon install stuck" linked from
      `provisioning-workflow-stuck.md`.
