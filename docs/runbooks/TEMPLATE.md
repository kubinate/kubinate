# Runbook — <Name>

**Severity**: SEV-<1|2|3|4>
**Alert source(s)**: <Alertmanager route / log-based alert / synthetic>
**Owner**: <team or named role (not a person)>
**Last reviewed**: YYYY-MM-DD

## Summary

One or two sentences describing what this runbook covers and when you'd
hit it. If you got paged and found this runbook, you should know within
15 seconds whether you're in the right place.

## Symptoms

- Specific, observable symptoms. "API `/readyz` returns 503 from the
  Cloudflare health check for > 2 minutes."
- Include the exact alert name(s) that fire.

## Severity rubric

When is this SEV-1 vs SEV-2 vs SEV-3? Make the call before engaging
broadly — escalate explicitly if you start in the wrong tier.

## Immediate actions (first 5 minutes)

1. Acknowledge the page so others know you're on it.
2. Open the incident channel: `#incident-YYYYMMDD-<short-slug>`.
3. Start the incident log (timestamped actions).
4. Stabilize before you investigate. If a quick rollback or feature
   flag flip restores service, do it *first* and investigate after.

## Diagnosis

Concrete commands, dashboard links, log queries. Not prose.

```bash
# Example: check the most recent deploy
kubectl -n kubinate rollout history deployment/api | tail -n 5
```

- Dashboard: <link>
- Logs: <Grafana/Loki link with pre-filled query>
- Traces: <Tempo link>

## Mitigations

Ordered by preference. For each: what it does, what it costs, how to
apply, how to verify.

1. **<mitigation>** — restores service, may cause <trade-off>.
   ```bash
   <exact command>
   ```

## Recovery / rollback

What "done" looks like and how to confirm.

## Communications

- Who to notify when (status page, customer-facing updates).
- Template lines for each channel.

## Postmortem

Required for SEV-1 and SEV-2. Target timeline: draft within 48h,
published within 5 business days. Use the postmortem template.

## Related

- ADRs: <links>
- Dashboards: <links>
- Past incidents: <links>
