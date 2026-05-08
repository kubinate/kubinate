# Runbook — Observability proxy is down or returning errors

**Severity**: SEV-3 (degraded — dashboards stale, no customer impact
            beyond observability blind-spot) → SEV-2 if it persists
            past one hour while a separate incident is in flight
            (we lose the ability to see what we're triaging).
**Alert source(s)**:
- `KubinateObservabilityWriteRejecting` (Prometheus) — sustained
  non-2xx response rate on `POST /v1/observability/write`.
- `KubinateObservabilityQueryLatency` (Prometheus) — p99 query
  latency above 5 s for > 10 minutes.
- Internal Slack report from on-call when their dashboards stop
  refreshing.
**Owner**: Observability team.
**Last reviewed**: 2026-05-04

## Summary

Sprint 3 ticket 07 ships the observability proxy as a thin scaffold
in front of an in-process metrics store
(`kubinate_observability::metrics::InMemoryMetricsStore` on
`kubinate-api`). Phase 4+ replaces this with VictoriaMetrics behind
the same trait (see
[`docs/decisions/sprint-3-observability-tsdb.md`](../decisions/sprint-3-observability-tsdb.md)).

This runbook covers two scenarios:

1. The proxy itself is rejecting writes or queries (likely an
   API-layer regression, or the in-process store is exhausted).
2. The proxy is up but the data is missing or wrong-tenant — the
   tenant-isolation invariant has broken.

Scenario (2) is the more serious one. **Treat any cross-tenant
data leak as a SEV-1 security incident**, follow the
"Tenant-isolation breach" section below, and engage the on-call
security maintainer before continuing.

## Symptoms

- `POST /v1/observability/write` returns `403` for legitimate
  callers — most likely `MetricsError::TenantMismatch` because the
  agent's claimed `organization_id` got out of sync with its session.
- `POST /v1/observability/query` returns the wrong tenant's
  samples or an empty array when the writer just succeeded.
- API memory grows without bound — the in-process store is the
  scaffold, not production; sustained traffic eats RAM.
- `/metrics` shows `kubinate_runner_workflows_inflight` healthy but
  the API process is still slow (rules out runner pressure).

## Severity rubric

- **SEV-3**: writes/queries failing for one tenant; the proxy is up
  for everyone else.
- **SEV-2**: proxy down for all tenants, OR a separate primary
  incident is in flight and the dashboards are stale.
- **SEV-1**: cross-tenant data leak observed. Engage security and
  follow the breach playbook below before any other action.

## Immediate actions (first 5 minutes)

1. Ack the page.
2. Scope the failure: is it one tenant, all tenants, or "wrong
   tenant"?
   ```bash
   # Hit /metrics directly to bypass any frontend caching.
   curl -s https://api.kubinate.example/metrics \
     | grep kubinate_observability_
   ```
3. If "wrong tenant," **stop here**, page security, freeze the
   suspect API binary version (`kubectl -n kubinate annotate
   deployment/api ops/incident=YYYYMMDD --overwrite`), and follow
   the "Tenant-isolation breach" section. Do not roll the binary
   forward without a security review.

## Diagnosis

### Common: 403 on write for a single tenant

The agent's request body claims an `organization_id` that doesn't
match the authenticated actor's tenant. Either the agent's session
was rotated and the body wasn't regenerated, or the API key was
re-issued to a different tenant.

```bash
# Check the most recent write rejections in the API log.
journalctl -u kubinate-api --since "5 minutes ago" \
  | grep "tenant mismatch"
```

The mitigation is on the agent side: regenerate the agent's
session and confirm the body's `organization_id` matches before
the next write.

### Common: write succeeds but query returns empty

The scaffold has no persistence — a process restart drops every
sample. Confirm:

```bash
ps -o etime= -p "$(pgrep kubinate-api)"
```

If the process started after the writes, that's expected — Phase 2
ships best-effort observability. The runbook for Phase 4+ when the
VictoriaMetrics-backed store lands will document a different
diagnosis path.

### Tenant-isolation breach (SEV-1)

If a query returns samples that belong to a different tenant:

1. **Freeze writes**: scale `kubinate-api` to zero replicas, OR
   block `/v1/observability/*` at the Cloudflare WAF, whichever
   the on-call team has practiced.
2. Capture the offending request id (`x-request-id` header) from
   the affected query response. Cross-reference it against the
   API access log to identify the specific user, tenant, and
   session.
3. Inspect the in-process store snapshot. Until the live VM
   backend is wired this is just a memory dump — but the unit
   test `round_trip_returns_only_the_writers_samples` should also
   be re-run and compared to the affected version of the binary.
4. Page security; follow the breach playbook in
   `docs/security/incident-response.md` (when written).

## Mitigations

Ordered by preference.

1. **Restart the API process** (SEV-3 / SEV-2). Drops the
   in-process store; expected behaviour for the scaffold.
   ```bash
   kubectl -n kubinate rollout restart deployment/api
   ```

2. **Roll back to the previous binary** (SEV-1 isolation breach).
   Only after security signs off; the rollback target must be
   known-good against the tenant-isolation tests.

3. **Disable observability writes temporarily**. The agent's
   reverse tunnel (when shipped) has a feature flag that suppresses
   metric pushes; toggle it via the agent management API. Until
   then, blocking `/v1/observability/write` at the WAF is the
   only knob.

## Recovery / rollback

- `kubinate_observability_query_latency_seconds_bucket{le="1"}`
  returns to ≥ 0.99 ratio.
- The `round_trip_returns_only_the_writers_samples` and
  `tenant_mismatch_in_write_body_is_rejected` tests pass on the
  recovered binary version.
- Affected tenant confirms the dashboards are populating again.

## Communications

- **SEV-3**: status note to the engineering channel; no external
  customer comms unless a specific tenant has been pinged.
- **SEV-2**: status page update once dashboards have been stale
  for > 30 minutes during another incident.
- **SEV-1 (isolation breach)**: follow the breach playbook —
  customer-facing comms are owned by the security maintainer, not
  the on-call engineer.

## Follow-ups (file the first time this runbook is exercised)

- Live `kubinate_observability_*` Prometheus gauges + counters on
  the proxy (today there are none).
- Wallclock test that asserts the proxy returns `403` *before*
  reaching the store on a tenant-mismatch (currently true by
  inspection but not under test at the API edge).
- A pre-commit guard (or `code-reviewer` agent prompt) that flags
  any new observability handler that takes `organization_id` from
  the request body rather than from `Actor::organization_id`.

## Related

- ADR-0006: Multi-tenancy isolation (the invariant this runbook
  defends).
- ADR-0011: Defer the Temporal SDK adoption (the metrics
  infrastructure this proxy lives on).
- `docs/decisions/sprint-3-observability-tsdb.md`: the spike doc
  that decides Mimir vs. VictoriaMetrics + Loki vs. S3.
- `crates/observability/src/metrics.rs`: the proxy's tenant guard.
