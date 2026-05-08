# Runbooks

One file per scenario, all in this directory. New runbooks copy
[`TEMPLATE.md`](./TEMPLATE.md) and follow that exact section order so a
paged engineer can scan a familiar layout under stress.

## Index

| Runbook | When to open |
|---|---|
| [`addon-install-stuck.md`](./addon-install-stuck.md) | A `cluster_addons` row has been in `installing` for too long. |
| [`agent-heartbeat-missing.md`](./agent-heartbeat-missing.md) | A user-cluster agent has stopped reporting in. |
| [`control-plane-redeploy.md`](./control-plane-redeploy.md) | Routine Ansible-driven deploy or rollback of `kubinate-api`. |
| [`credential-rotation.md`](./credential-rotation.md) | Tenant Hetzner-token compromise or customer-requested rotation. |
| [`db-pool-exhaustion.md`](./db-pool-exhaustion.md) | Postgres connection pool saturated; API returning 503s. |
| [`hetzner-5xx-surge.md`](./hetzner-5xx-surge.md) | Hetzner Cloud API is returning a sustained surge of 5xx — decide whether to pause provisioning. |
| [`observability-proxy-down.md`](./observability-proxy-down.md) | The metric/log query proxy is down, slow, or returning the wrong tenant's data. |
| [`owner-passkey-lost.md`](./owner-passkey-lost.md) | An Owner / Admin lost their passkey and / or recovery codes; one of three paths back to a working session. |
| [`provisioning-workflow-stuck.md`](./provisioning-workflow-stuck.md) | A `ProvisionClusterWorkflow` is looping or stuck on a single activity. |
| [`sse-connection-leak.md`](./sse-connection-leak.md) | `/v1/clusters/:id/events` streams are not closing after their cluster's workflow terminates. |

## Conventions

- Every runbook opens with **Severity**, **Alert source(s)**, **Owner**,
  **Last reviewed**. The "Last reviewed" date matters — anything older
  than six months is treated as suspect on call.
- The **Immediate actions (first N minutes)** section must contain
  copy-pasteable commands. Prose belongs further down.
- Cross-link related runbooks in the **Related** section so an engineer
  who opened the wrong one can navigate sideways without searching.

## Adding a new runbook

1. `cp TEMPLATE.md <slug>.md`.
2. Fill in every section. Empty sections rot fastest — delete the
   heading rather than leaving a `TODO`.
3. Add a row to the table above and a backlink from any related
   runbook's `Related` section.
4. Open a PR and request review from a second engineer; the
   second-engineer review is part of the runbook's Definition of Done.
