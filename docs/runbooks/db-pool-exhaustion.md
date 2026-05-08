# Runbook — Database connection pool exhaustion

**Severity**: SEV-1 (API is hard-down) / SEV-2 (degraded)
**Alert source(s)**:
- `KubinateApiConnectionPoolExhausted` (Prometheus: `sqlx_pool_idle == 0` for > 2m)
- `KubinateApiP99LatencyHigh` (often a symptom, not a cause)
**Owner**: Platform team.
**Last reviewed**: 2026-04-24

## Summary

The API process cannot acquire a Postgres connection within
`acquire_timeout`, and requests are failing or timing out. Almost
always one of: a runaway query holding transactions open, a pool too
small for current traffic, or Postgres itself struggling.

## Severity rubric

- **SEV-1**: `/readyz` failing for the majority of replicas; customer
  dashboard unusable.
- **SEV-2**: elevated 5xx rate but majority of requests succeed.

## Immediate actions (first 5 minutes)

1. Ack the page. Open incident channel.
2. Check the Prometheus panel "DB pool saturation" and the Postgres
   "active connections" panel side by side.
3. **Do not** restart the API pods yet — restarting masks the
   diagnosis. Do restart if Postgres is clearly fine and there's
   evidence of a stuck API process (e.g. goroutines/tasks stuck on
   one host only).

## Diagnosis

Find long-running queries:

```sql
SELECT pid, now() - xact_start AS xact_age, state, query
FROM pg_stat_activity
WHERE state <> 'idle'
  AND xact_start < now() - interval '30 seconds'
ORDER BY xact_age DESC
LIMIT 20;
```

Find transactions idle-in-transaction (these are the usual culprit —
a request handler that `BEGIN`'d and didn't `COMMIT`/`ROLLBACK`):

```sql
SELECT pid, now() - state_change AS idle_age, application_name, query
FROM pg_stat_activity
WHERE state = 'idle in transaction'
ORDER BY idle_age DESC
LIMIT 20;
```

Check Postgres connection count vs `max_connections`:

```sql
SHOW max_connections;
SELECT count(*) FROM pg_stat_activity;
```

Check the API-side pool metrics (Prometheus):

- `sqlx_pool_size` — configured max (should be 20 per replica in v1).
- `sqlx_pool_idle` — idle connections. Zero-for-long = exhaustion.
- `sqlx_pool_connections_waiting` — waiters.

## Common causes

| Signal | Cause | Fix |
|---|---|---|
| `idle in transaction` rows for > 1m | Handler forgot to commit/rollback (often an early return before `TenantScopedTransaction::commit`) | Terminate the offending backend(s), identify the handler, patch. |
| Long-running `SELECT` on audit table | Missing index or tenant ID not pushed down into the query plan | EXPLAIN the query, add index, revisit RLS policy plan. |
| Pool size too small for traffic | Real capacity issue | Scale horizontally (more API replicas) first; raise `max_connections` second. |
| Many idle connections but no waiters | Not an exhaustion issue — check the alert. | Confirm `sqlx_pool_connections_waiting`. |

## Mitigations

1. **Kill an idle-in-transaction backend** (if clearly stuck):
   ```sql
   SELECT pg_terminate_backend(<pid>);
   ```
   Targeted kills only. Do not mass-kill.

2. **Scale API replicas** to shed load from the current pods:
   ```bash
   kubectl -n kubinate scale deployment/api --replicas=6
   ```

3. **Restart API replicas** (releases all connections). Last resort
   and only if diagnosis points to an API-side leak.
   ```bash
   kubectl -n kubinate rollout restart deployment/api
   ```

4. **Raise `max_connections`** in Postgres: requires restart in
   non-CNPG setups; in CNPG, edit the Cluster object and let the
   operator roll. Do not do this as a first response — it masks
   leaks.

## Recovery / rollback

- `sqlx_pool_idle > 5` sustained for 5 minutes.
- `/readyz` green across all API replicas.
- No `idle in transaction` older than 10 seconds.

## Communications

- SEV-1: status page update within 5 minutes.
- SEV-2: within 15 minutes if customer-visible errors persist.

## Postmortem

Required for SEV-1 and SEV-2. Root cause must name whether this was
an API bug, a query plan regression, or a capacity shortfall, and
what preventive control catches the next instance earlier.

## Related

- ADR-0005: Primary database.
- ADR-0006: Multi-tenancy (RLS query plans).
- Dashboard: `Kubinate / Database`.
