# Runbook — Provisioning workflow stuck in retry loop

**Severity**: SEV-2 (customer-visible provisioning failure) /
            SEV-3 (single-tenant, no business impact)
**Alert source(s)**:
- `KubinateProvisioningWorkflowRetrying` (Prometheus) — a provisioning
  workflow has > 5 activity retries in a 10-minute window.
- Customer support ticket tagged `stuck provisioning`.
**Owner**: Cluster Orchestrator team.
**Last reviewed**: 2026-05-03

## Summary

The `ProvisionClusterWorkflow` is making progress in starts-and-stops
or looping on a specific activity. User sees a stalled provisioning
bar in the dashboard. Most common cause: transient Hetzner API errors
or SSH timing issues during k3s bootstrap.

This runbook also covers the **scaling** case (Sprint 3 ticket 08):
`ScaleOutWorkflow` and `ScaleInWorkflow` reuse the same activity
catalog (Hetzner create/delete + SSH k3s join + kubectl drain) and
fail in the same shapes. A cluster stuck in `status = scaling` is
covered by the diagnosis and mitigation steps below — read the
"Scaling-specific notes" callout for the bits that differ.

## Symptoms

- Alert fires with `workflow_id` label.
- Dashboard shows the workflow in `Running` status for > 15 minutes on
  a single-node cluster, or > 25 minutes on HA.
- Temporal UI shows the same activity retrying with backoff.
- User's dashboard shows provisioning progress stuck at a specific
  step (e.g. "installing k3s server").

## Severity rubric

- **SEV-2**: multiple tenants affected concurrently, or a single
  alpha/enterprise tenant is affected.
- **SEV-3**: single free-tier tenant; no evidence of broader Hetzner
  incident.

## Immediate actions (first 5 minutes)

1. Ack the page.
2. Check the Hetzner Cloud status page
   (https://status.hetzner.com/) — if Hetzner is reporting an
   incident in the affected region, link it into the incident channel
   and update the Kubinate status page accordingly.
3. Open the Temporal UI and find the workflow by `workflow_id` from
   the alert label.

## Diagnosis

Find the workflow:

```bash
# From the control plane host:
temporal workflow describe \
  --workflow-id "$WORKFLOW_ID" \
  --namespace kubinate
```

Identify the failing activity and its error classification:

```bash
temporal workflow show \
  --workflow-id "$WORKFLOW_ID" \
  --namespace kubinate \
  --fields long
```

Common causes and signatures:

| Symptom in activity error | Likely cause | Mitigation |
|---|---|---|
| `429 Too Many Requests` from Hetzner | Rate limit | Back-off is already automatic; wait one window. If sustained, see below. |
| `ssh: handshake failed` | VPS not yet fully booted / wrong fingerprint | Extend `SshProvisionK3sServer` timeout per the workflow signal; inspect Hetzner console for boot errors. |
| `k3s server failed to start` | Bad k3s arg, disk full, or upstream k3s image fetch failed | See the k3s journal on the node; common cause is transient registry.k8s.io timeouts. |
| `etcd quorum not reached` | Private network misconfig between regions | Re-check the Hetzner private network attachment; may need to recreate. |
| `kubectl drain: node X is unschedulable` (scale-in only) | PodDisruptionBudget blocking eviction | Inspect PDBs in the affected namespace; either temporarily relax or scale-in a different worker (highest-index-first is just the default). |
| `kubectl: timeout` after 240s on drain (scale-in only) | A pod refuses to terminate (finalizer or stuck volume) | Get the offending pod from `kubectl get pods --field-selector=spec.nodeName=<node>`; force-delete if appropriate, then re-POST `/v1/clusters/:id/workers` with `delta: 0` to no-op confirm and retry the original delta. |

## Scaling-specific notes (Sprint 3 ticket 08)

- **Status sequence**: `ready → scaling → ready` on success, or
  `ready → scaling → failed` on workflow error. A cluster stuck in
  `scaling` is exactly the case this runbook covers — treat it the
  same as a stuck provision.
- **Idempotency**: scale-in retries are safe because both the
  `kubectl` wrapper and `hetzner_delete_server_idempotent` map
  `NotFound` to success. Re-running a scale-in workflow on a
  partially-completed run will not double-delete.
- **Worker selection**: scale-in picks the highest-index workers
  first (newest hostnames `acme-prod-worker-NN` with the largest
  `NN`). If a specific worker must be kept, drain it manually first
  and run scale-in by `delta` only after — there is no per-worker
  selector in the API yet.
- **Worker count drift**: the `clusters.worker_count` column is
  updated by the API handler *before* the workflow starts. If the
  workflow fails halfway, that column will overstate reality until
  an operator either retries the scale or runs `UPDATE clusters SET
  worker_count = (SELECT count(*) FROM cluster_servers WHERE
  cluster_id = ... AND role = 'worker' AND deleted_at IS NULL)`.
  Use a single tenant-scoped tx (`SET LOCAL
  app.current_tenant_id`).

## Mitigations

Ordered by preference.

1. **Wait out transient errors** (Hetzner 5xx, 429). The workflow's
   retry policy handles this; if it has not yet exceeded
   `max_attempts`, do nothing. Inform the user via status page update
   that their provisioning is delayed.

2. **Signal the workflow to retry a specific activity**:
   ```bash
   temporal workflow signal \
     --workflow-id "$WORKFLOW_ID" \
     --name retry-activity \
     --input '{"activity_id": "SshProvisionK3sServer"}'
   ```

3. **Cancel and restart** (user-visible: their provisioning will
   fully restart, orphaned Hetzner resources will be cleaned up by
   the compensation):
   ```bash
   temporal workflow cancel --workflow-id "$WORKFLOW_ID"
   # Then, from the application DB, mark the cluster row
   # provisioning_state = 'queued' for a fresh start.
   ```

4. **Hetzner-side investigation** — if the Hetzner console shows the
   VPS in an error state, the VPS may need manual power-cycle or
   deletion. Log each step in the incident channel.

## Recovery / rollback

- Workflow reaches `Completed` status in Temporal UI.
- Cluster appears `Ready` in the dashboard.
- `/readyz` on the cluster's kube-apiserver (via the agent) succeeds
  for 5 consecutive checks.

## Communications

- If > 5 tenants affected at once: update status page to "Investigating
  — elevated provisioning failures" within 10 minutes.
- Individual affected tenant: send a templated apology + explanation
  once the cluster is Ready.

## Postmortem

Required for SEV-2. Focus on whether the retry policy caught the
transient correctly and whether the dashboard signal to the user was
clear during the delay.

## Related

- [hetzner-5xx-surge.md](./hetzner-5xx-surge.md) — when the activity
  errors are upstream Hetzner 5xx rather than our side, start there.
- [addon-install-stuck.md](./addon-install-stuck.md) — for an addon
  install (Sprint 2 ticket 07) that never reaches `ready`.
- ADR-0003: Cluster provisioning orchestration.
- Dashboard: `Kubinate / Provisioning workflows`.
- Temporal UI: `https://temporal.ops.kubinate.com/namespaces/kubinate/workflows`.
