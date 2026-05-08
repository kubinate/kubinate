# Show cluster status on the dashboard with real-time updates

**Labels**: `area/frontend`, `area/api`, `sprint-1`
**Size**: 3

## Context

User needs to see provisioning progress in real time. Polling is fine
for Sprint 1 (2-second interval against a status endpoint). WebSocket
or SSE is Sprint-3+ scope.

## Acceptance criteria

- **Given** a provisioning cluster, **when** the user views its page,
  **then** the UI polls `/v1/clusters/:id` every 2s and shows: overall
  state, current step, elapsed time, estimated remaining time.
- **Given** a ready cluster, **then** polling stops and the page
  shows the "Download kubeconfig" button plus basic cluster facts.
- **Given** a failed provisioning, **then** the page shows the user-
  facing error category (not raw Hetzner strings, see Threat model
  Flow 3) and a "Retry" action.

## DoD
- [ ] No polling after terminal state
- [ ] Error message mapping tested
