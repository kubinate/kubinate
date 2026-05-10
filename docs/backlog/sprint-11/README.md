# Sprint 11 — Addon Uninstall

## Goal
Allow owners/admins to uninstall a ready addon from the cluster detail page. The
`Uninstalling` and `Uninstalled` states already exist in the SQL enum and domain
model; this sprint wires the plumbing end-to-end.

## Tickets

### 11-01 `HelmExecutor::uninstall` trait method + `HelmCliExecutor` impl
- Add `uninstall(kubeconfig_path, release, namespace) → Result<(), HelmError>` to the trait.
- `helm uninstall <release> --namespace <ns> --wait --timeout 5m`.
- Idempotent: treat `"release: not found"` stderr as success.

### 11-02 `AddonRepository::get_by_id` + `PgAddonRepository` impl
- `get_by_id(org_id, id) → Result<Option<ClusterAddon>, PlatformError>`.
- RLS enforced via `SET LOCAL` inside a transaction.

### 11-03 `AddonService::get_by_id` + runner `spawn_uninstall_addon`
- Thin service wrapper around the repository.
- Runner: `run_uninstall_addon` (mirrors `run_install_addon`):
  transitions `uninstalling → uninstalled` on success, `failed` on error.
- `spawn_uninstall_addon` spawns on the tokio runtime with an inflight guard.

### 11-04 `DELETE /v1/clusters/:id/addons/:addon_id` API route
- Gated on `OwnerActor`.
- Look up addon, verify it belongs to this cluster and is in `ready` or `failed` state.
- Write `status = uninstalling` synchronously, then spawn the background task.
- Return `202 Accepted` with the updated `AddonView`.

### 11-05 Frontend uninstall button + `uninstallAddon` API helper
- `uninstallAddon(clusterId, addonId)` in `addons.ts`.
- Uninstall button on each `ready` addon row in the cluster detail page.
- Per-row `uninstallInFlight` and inline error; refreshes the addon list on success.
