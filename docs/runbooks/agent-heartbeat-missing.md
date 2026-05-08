# Runbook — Agent lost contact with control plane

**Severity**: SEV-3 (single cluster) / SEV-2 (many clusters at once)
**Alert source(s)**:
- `KubinateAgentHeartbeatMissing` (no heartbeat for > 5m)
- Customer ticket: "my dashboard says the cluster is offline"
**Owner**: Agent team.
**Last reviewed**: 2026-04-24

## Summary

The kubinate-agent in a user's cluster has not reported in for > 5
minutes. The cluster may still be healthy — the customer's workloads
are unaffected — but our observability into it is gone, and
control-plane-initiated operations (add-on installs, upgrades) will
fail or queue.

## Severity rubric

- **SEV-2**: > 1% of connected clusters lose heartbeat in a 10-minute
  window. Almost always a control-plane side problem.
- **SEV-3**: one cluster. Usually a customer-side network or RBAC
  change.

## Diagnosis

### Is it many clusters or one?

Prometheus: `count(up{job="kubinate-agent"} == 0)` vs.
`count(up{job="kubinate-agent"})`.

- Ratio > 1% → treat as SEV-2, investigate control plane first.
- Single cluster → treat as SEV-3, investigate that cluster.

### SEV-2 path (control plane)

1. Check ingress to the agent endpoint:
   ```bash
   curl -v https://agent.kubinate.com/healthz
   ```
   The endpoint is mTLS-only; you should get TLS negotiation failure
   without a client cert. A connection refused or timeout indicates
   the ingress itself is the problem.

2. Check the control plane side of the reverse tunnel:
   - Metric: `kubinate_agent_active_connections` — should track active
     cluster count.
   - Logs for `agent-ingress` service — filter for TLS errors, cert
     rotation events.

3. Recent deploys on the control plane. The most common cause of a
   mass heartbeat loss is a control-plane restart without connection
   draining.

### SEV-3 path (single cluster)

Get the cluster's identity:

```bash
# From the Kubinate DB (use the bypass role; log the reason):
SELECT id, organization_id, name, region
FROM clusters
WHERE id = :cluster_id;
```

Check recent agent logs via the control plane's view (we keep the last
1 hour of agent logs even when the agent is offline, because they are
pushed independently):

```
# Grafana / Loki:
{cluster_id="<id>"} |= "kubinate-agent" | json
```

Most frequent causes:

| Signal | Cause | Mitigation |
|---|---|---|
| Last log is `TLS: certificate expired` | Agent cert not rotated | Trigger rotation workflow (see below). |
| Last log is `connection reset` and recovers repeatedly | Customer network blip | Nothing to do; watch. |
| No logs for > 15m, cluster otherwise healthy | Agent pod OOM or evicted | Ask customer to check DaemonSet status; if we have the permission, fetch it ourselves. |
| Customer ticket mentions "firewall change" | Egress blocked | Point customer at our egress allowlist docs. |

## Mitigations

**Rotate the agent certificate** (if the alert cause is expiry or
revocation):

```bash
# From the control plane:
kubinate-admin agent rotate-cert --cluster-id "$CLUSTER_ID"
```

This issues a new cert and signals the agent over its existing tunnel;
if the tunnel is gone, the rotation is queued and picked up on
reconnect.

**Manually restart the agent** (customer-mediated, over their kubectl):

Send them the command:

```bash
kubectl -n kubinate-system rollout restart daemonset/kubinate-agent
```

## Recovery / rollback

- Heartbeat resumes; `up{job="kubinate-agent", cluster_id="$ID"} == 1`.
- Dashboard shows cluster as "connected".

## Communications

- SEV-2: status page "Investigating — degraded observability".
- SEV-3: in-dashboard banner on the affected cluster explaining the
  situation. Email only if persists > 1 hour.

## Postmortem

Required for SEV-2 only.

## Related

- ADR-0009 / ADR-0010: Auth and authorization (mTLS cert lifecycle).
- Threat model, Flow 5.
- Agent egress allowlist docs.
