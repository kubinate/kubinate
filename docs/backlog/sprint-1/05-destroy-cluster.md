# Implement DestroyClusterWorkflow

**Labels**: `area/workflows`, `area/cluster`, `sprint-1`
**Size**: 5

## Context

Symmetric to provisioning. Idempotent: if some servers already do not
exist, the workflow reports success. Must leave no orphans in the
user's Hetzner project.

## Acceptance criteria

- **Given** a cluster in any state (provisioning, ready, errored),
  **when** `DestroyClusterWorkflow` runs, **then** all servers,
  load balancers, and private networks created by us are deleted
  from the user's Hetzner project.
- **Given** a partially-destroyed cluster and a re-run, **then** the
  workflow completes successfully without errors about missing
  resources.
- **Given** a destroyed cluster, **then** the kubeconfig handle is
  deleted from the secret store and the cluster row is soft-deleted.

## DoD
- [ ] Integration test: provision → destroy → assert zero residual
      resources via Hetzner API listing
- [ ] Audit entries for destroy lifecycle events
