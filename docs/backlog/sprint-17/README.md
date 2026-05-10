# Sprint 17 — Live Cluster List + Search/Filter

## Goal
Make the cluster list useful while clusters are transitioning: status
updates appear automatically every 3 seconds without a manual reload,
and users can narrow the list by name or status.

## Tickets

### 17-01 Live cluster list polling
- `clusters` state initialised from the SSR-loaded `data.clusters` via
  `untrack()` to avoid a reactive dependency on the load data.
- `onMount` fetches a fresh list immediately after hydration.
- `$effect` watches `hasTransient` (any cluster with status
  `pending | provisioning | scaling | destroying`); starts a 3-second
  `setInterval` poll while true, stops it when all clusters reach a
  terminal state.
- `onDestroy` clears the interval to avoid memory leaks.
- Transient badges receive `animate-pulse` so users can see at a glance
  which clusters are still moving.

### 17-02 Search + status filter chips
- Text input (max-w-xs, placeholder "Search clusters…") filters the grid
  by cluster name (case-insensitive).
- Status chip buttons appear for each status present in the list (sorted:
  provisioning → scaling → ready → failed → destroyed). Multiple chips
  can be active simultaneously (OR logic). Chips only appear when ≥ 2
  statuses exist.
- `filteredClusters` derived value applies search AND active-status
  filters; empty-match state shows "No clusters match your filter."
  Stats row always reflects the full unfiltered list.
