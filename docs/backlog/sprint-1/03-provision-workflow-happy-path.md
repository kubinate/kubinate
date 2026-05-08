# Implement ProvisionClusterWorkflow happy path in Temporal

**Labels**: `area/workflows`, `area/cluster`, `area/integrations`, `sprint-1`
**Size**: 8

## Context

The core of Phase 1. See ADR-0003. This issue covers the happy-path
only — compensations and partial failure are issue #05 (destroy) and
follow-up work in Sprint 2.

## Activities to implement

1. `HetznerCreateServer(server_type, region, ssh_key, user_data) -> ServerId`
2. `WaitForCloudInit(server_id) -> Ready`
3. `SshInstallK3sServer(server_id, k3s_version) -> JoinToken`
4. `SshInstallK3sAgent(server_id, join_token, control_plane_ip) -> NodeId`
5. `CollectKubeconfig(control_plane_id) -> Kubeconfig` (feeds into #04)

## Acceptance criteria

- **Given** a valid `ProvisionClusterInput`, **when** the workflow
  runs to completion, **then** a k3s cluster exists on the user's
  Hetzner project, with N worker nodes joined, in < 10 minutes (P50).
- **Given** a workflow replay, **when** replaying from history,
  **then** the workflow produces the same result deterministically
  (tested with Temporal's replay test helpers).
- **Given** a 429 from Hetzner, **when** the activity retries, **then**
  retries follow exponential backoff with jitter and do not exceed
  5 attempts.

## DoD
- [ ] Workflow replay test
- [ ] Unit tests for each activity (mock Hetzner client)
- [ ] Integration test against a Hetzner test project behind a
      feature-flagged CI job
- [ ] Shadow table `provisioning_workflows` updated at each step
- [ ] Runbook referenced for the "stuck in retry" case
