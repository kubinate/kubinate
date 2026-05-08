# Runbook — Hetzner Cloud API 5xx surge

**Severity**: SEV-2 (multi-tenant provisioning failures) /
            SEV-3 (single tenant affected, narrow region)
**Alert source(s)**:
- `KubinateHetznerApiErrorRateHigh` (Prometheus) — Hetzner API 5xx
  rate exceeds 5% of requests over a 5-minute window.
- `KubinateHetznerApiBudgetBurn` — error-budget burn rate breaching the
  fast-burn threshold for the `hetzner_api` SLO.
- Pages from synthetic provisioning canary failures.
**Owner**: Cluster Orchestrator / SRE rotation.
**Last reviewed**: 2026-04-25

## Summary

Hetzner Cloud's REST API is returning a sustained surge of 5xx
responses (502 / 503 / 504, occasionally 500). The Kubinate
provisioning and destroy workflows both depend on this API; while
short blips are absorbed by activity retry policies, a sustained
surge causes user-visible provisioning failures and can pile up
orphan resources in customer Hetzner projects.

This runbook lets you, in under 60 seconds:
1. Confirm Hetzner is in fact incident-state (vs. our credentials,
   our network).
2. Decide whether to pause provisioning workflows.
3. Find the exact pause / resume command.

If you need the deeper "stuck workflow" fixes, jump to
[provisioning-workflow-stuck.md](./provisioning-workflow-stuck.md).

## Symptoms

- One or both of the alerts above firing on a `provider=hetzner` label.
- `cluster.status_reason` on newly failed clusters starts with
  `hetzner: 5xx`.
- Temporal UI shows `HetznerCreateServer` / `HetznerDeleteServer`
  retrying with exponential backoff against the same set of Hetzner
  endpoints.
- Customer reports of "stuck at creating control plane".

## Severity rubric

- **SEV-2**: > 5% of provisioning workflows in the last 30 minutes
  have failed, *or* an enterprise / paying tenant is affected, *or*
  Hetzner reports a region-wide incident overlapping ours.
- **SEV-3**: < 5% of workflows affected, no Hetzner status-page entry,
  no paying tenant impact. Often resolves inside the existing retry
  budget without intervention.

## Immediate actions (first 60 seconds)

1. **Confirm Hetzner is the cause** — open the Hetzner status page:
   https://status.hetzner.com/
   - If they have an active "Cloud API" or relevant region incident,
     link it into the incident channel and continue with this runbook.
   - If their status page is green, this may be us — pivot to the
     provisioning-workflow-stuck runbook to investigate our side.

2. **Check our error rate** at:
   https://grafana.ops.kubinate.com/d/hetzner-api-errors
   (filter to last 30 min; look for the spike onset).

3. **Decide whether to pause provisioning**. Pause if any of:
   - Hetzner posts an active "investigating" incident.
   - Our error rate is > 25% sustained for > 5 minutes.
   - More than 3 distinct tenants affected concurrently.

   Pause command (control-plane host):

   ```bash
   # Set the pause flag — the API rejects new POST /v1/clusters with
   # 503 + "provisioning paused" while this is set, and the workflow
   # worker stops dequeuing new ProvisionClusterWorkflow runs (in-flight
   # workflows continue under their own retry policy).
   kubinate-ops feature-flag set provisioning.paused true \
     --reason "hetzner-5xx-surge $(date -Iseconds)"
   ```

   Resume command:

   ```bash
   kubinate-ops feature-flag set provisioning.paused false
   ```

4. **Open the incident channel** `#incident-YYYYMMDD-hetzner-5xx` and
   start the timestamped log.

## Diagnosis

Confirm the upstream is the problem (and not, say, a Cloudflare
Tunnel issue between us and Hetzner):

```bash
# From any control-plane host. The token must be a read-only
# operator token, never a tenant token.
curl -sS -o /dev/null -w "%{http_code}\n" \
  -H "Authorization: Bearer $KUBINATE_HETZNER_OPERATOR_TOKEN" \
  https://api.hetzner.cloud/v1/locations
```

- `200` repeatedly → Hetzner is fine; this is *our* problem. Switch to
  [provisioning-workflow-stuck.md](./provisioning-workflow-stuck.md).
- `5xx` repeatedly → confirmed upstream surge, continue here.
- `401 / 403` → operator token broken; rotate before continuing.

Affected workflows (Temporal UI or CLI):

```bash
temporal workflow list \
  --namespace kubinate \
  --query 'WorkflowType="ProvisionClusterWorkflow" AND ExecutionStatus="Running"' \
  | head -20
```

Per-activity error breakdown (last 30 min):

- Grafana: `Kubinate / Hetzner API` board → "Errors by endpoint".
- Logs (Loki): `{service="kubinate-api", level="error"}
  |= "hetzner" |= "5"` filtered to last 30 min.

## Mitigations

Ordered by preference. Smaller / reversible blast radius first.

1. **Wait one retry window** (5 min). The workflow's exponential
   backoff with jitter handles short blips; if Hetzner recovers we
   never need to act. Watch the error-rate panel for trend.

2. **Pause new provisioning** (see step 3 above) without cancelling
   in-flight workflows. Existing provisions ride out the surge under
   their own retry budget; new submissions are rejected with a clear
   message via the API. **This is the standard Sprint 1 mitigation.**

3. **Reduce concurrency** if Hetzner's incident is rate-limit-shaped
   rather than full outage. Drops our parallel `HetznerCreateServer`
   load.

   ```bash
   kubinate-ops worker scale provisioning --max-concurrent-activities 2
   ```

   Restore to default once recovered:

   ```bash
   kubinate-ops worker scale provisioning --max-concurrent-activities 8
   ```

4. **Cancel and reschedule severely stuck workflows** — only after
   Hetzner recovers, otherwise the destroy compensation path will
   fight the same surge. See `provisioning-workflow-stuck.md` §3 for
   the exact `temporal workflow cancel` invocation.

## Recovery / rollback

- Hetzner status page back to green; our error rate < 1% for 10
  consecutive minutes.
- Resume command run; verify a synthetic provisioning canary completes
  end-to-end.
- Any clusters in `failed` status because of the surge are
  destroyed-and-recreated by the affected tenant (or, for paying
  tenants, by us with explicit consent).

## Communications

- **t+5 min**: post "investigating — elevated provisioning failures,
  upstream provider (Hetzner) is reporting issues; new cluster
  creation may be paused" on the Kubinate status page if pause was
  triggered or > 3 tenants affected.
- **t+30 min** and every 30 min thereafter: status update with the
  current Hetzner incident link and our mitigation state.
- **Resolution**: status page → resolved with a 1-line summary; full
  postmortem within 5 business days for SEV-2.

## Postmortem

Required for SEV-2. Focus on:
- Did our retry policy / pause decision match what actually happened?
- Were any orphan Hetzner resources left in customer projects after
  the surge? (Customer-visible — non-zero is a regression on
  ADR-0003's "leave no orphans" property.)
- Should the SLO error-budget burn-rate alert have fired earlier?

## Related

- [provisioning-workflow-stuck.md](./provisioning-workflow-stuck.md)
  — when the symptoms look like a surge but are actually our side.
- ADR-0003: Cluster provisioning orchestration (retry-policy rationale).
- ADR-0004: Deployment topology (single Hetzner VPS, hard provider
  dependency).
- Dashboard: `Kubinate / Hetzner API`.
- Hetzner status page: https://status.hetzner.com/
