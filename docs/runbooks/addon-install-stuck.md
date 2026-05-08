# Runbook — Add-on install stuck

**Severity**: SEV-3 (single tenant affected) /
            SEV-2 (paying customer / multiple tenants affected)
**Alert source(s)**:
- `KubinateAddonInstallStuck` (Prometheus) — a `cluster_addons` row
  has been in `installing` status for > 10 minutes.
- Customer support ticket: "addon install spinner forever".
**Owner**: Cluster Orchestrator team.
**Last reviewed**: 2026-04-27

## Summary

A user requested an add-on install (Sprint 2 ticket 07 ships
ingress-nginx as the first one) but the row never transitioned past
`installing`. Most common causes: cluster API unreachable, Helm chart
fetch failed, chart version doesn't exist, or the controller's pods
are in `CrashLoopBackOff` and never become Ready before the workflow's
5-minute Helm `--wait` deadline.

## Symptoms

- Cluster status page shows a pending row in the addon list.
- `cluster_addons.status = 'installing'` and `status_reason` is NULL.
- The runner log emits `addon install run errored` with a `helm:`-
  prefixed reason, **or** the runner appears stuck on the
  `installing` step with no progress.

## Severity rubric

- **SEV-3**: Single free-tier tenant, one cluster, no follow-on
  incidents.
- **SEV-2**: Paying customer, or > 3 tenants stuck on the same chart
  version (suggests upstream chart regression).

## Immediate actions (first 60 seconds)

```bash
# 1. Find the affected addon row.
psql "$KUBINATE_DATABASE_URL" -c "
  SELECT id, cluster_id, addon, version, status, status_reason,
         created_at, updated_at
  FROM cluster_addons
  WHERE id = '<addon_uuid>';
"

# 2. Pull the runner's recent log lines for that addon.
journalctl -u kubinate-api -n 200 --grep "addon" \
  | rg "<addon_uuid>"
```

3. **Decide whether to fail the row** so the user can retry. If the
   `status_reason` already shows a `helm:` error, the runner already
   surfaced it; mark `failed` so the UI's retry button activates:

   ```sql
   UPDATE cluster_addons
   SET status = 'failed', status_reason = 'manual: stuck install'
   WHERE id = '<addon_uuid>';
   ```

## Diagnosis

If the runner is genuinely wedged (no recent log activity for that
addon id), check:

1. **Is the cluster reachable?**
   ```bash
   # The kubeconfig the runner used is gone (we delete the tempfile
   # right after the helm call), but every cluster has its kubeconfig
   # in the secret store. Pull a fresh one for inspection:
   kubinate-ops cluster kubeconfig --cluster-id <cluster_uuid> \
     --output /tmp/kc.yaml
   kubectl --kubeconfig /tmp/kc.yaml get nodes
   ```

2. **Did Helm fetch the chart?**
   ```bash
   helm --kubeconfig /tmp/kc.yaml list -A
   helm --kubeconfig /tmp/kc.yaml status <release>
   ```

3. **Are the controller pods Ready?**
   ```bash
   kubectl --kubeconfig /tmp/kc.yaml -n <namespace> get pods
   kubectl --kubeconfig /tmp/kc.yaml -n <namespace> describe pod <pod>
   ```

### CRD-ordering edge case (cert-manager)

cert-manager's chart ships CRDs that the controller depends on. We
install them via `crds.enabled: true` in the catalog default values
([`crates/addons/src/catalog.rs`](../../crates/addons/src/catalog.rs)),
which lets Helm sequence CRDs **before** the controller. Symptoms when
this is flipped off (or when the value is overridden by an operator):

- `helm install` returns success.
- Controller pods immediately CrashLoopBackOff with `no kind
  "Certificate" is registered`.
- The cert-manager namespace shows the deployment Ready=False.

Recovery:

```bash
# Re-install with CRDs explicitly enabled.
helm --kubeconfig /tmp/kc.yaml uninstall cert-manager -n cert-manager
# (Re-trigger via the UI — the catalog values include crds.enabled.)
```

Never apply the upstream CRDs manually with `kubectl apply -f` against
a Helm-managed release: the next chart upgrade fails because Helm
doesn't own the CRDs.

## Mitigations

Ordered by preference.

1. **Wait for the runner's 5-minute Helm `--wait` deadline** to fire.
   If `helm:` shows up in `status_reason`, the runner already gave up
   and marked the row failed; the user can retry from the UI.

2. **Manual `helm uninstall` + retry**:

   ```bash
   helm --kubeconfig /tmp/kc.yaml uninstall <release> -n <namespace>
   ```

   Then mark the row `failed` so the user's retry attempts are not
   blocked by the idempotency check.

3. **Pin a known-good chart version** if the customer hit an upstream
   regression. Update the catalog default in `crates/addons/src/catalog.rs`
   and ship a new binary; old `installing` rows can stay as-is — the
   user will retry with the new default next.

## Recovery / rollback

- `cluster_addons.status` is `ready` and `helm status` reports
  `deployed`.
- Cluster status page renders the addon as ready.
- If the user retried and the second attempt also failed at the same
  step, escalate to SEV-2 and capture controller pod logs in the
  incident channel.

## Communications

- SEV-3: respond on the support ticket once recovered.
- SEV-2: status page update if > 3 tenants affected; coordinate with
  customer comms on the chart-version pin if the upstream is at fault.

## Related

- [`provisioning-workflow-stuck.md`](./provisioning-workflow-stuck.md) — when
  the cluster itself never reached Ready and the addon install was
  rejected with `cluster must be Ready before installing addons`.
- [`hetzner-5xx-surge.md`](./hetzner-5xx-surge.md) — when Helm can't
  reach the cluster because the cluster's control plane is unreachable
  (Hetzner-side outage, not addon-side).
- ADR-0003: Cluster provisioning orchestration (workflow / activity
  decomposition shared with the addon path).
- Sprint 2 ticket 07 — addon catalog allowlist and Helm executor.
