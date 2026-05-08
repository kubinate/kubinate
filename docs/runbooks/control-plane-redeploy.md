# Runbook — Control-plane redeploy / rollback

**Severity**: SEV-2 (paying tenants affected during the window) /
            SEV-3 (alpha tenants only, scheduled).
**Alert source(s)**:
- Manual: an engineer is shipping a new `kubinate-api` binary.
- `KubinateApiUnhealthy` (Prometheus) — `/readyz` returning 503 for
  > 2 minutes after a deploy.
**Owner**: Cluster Orchestrator / SRE rotation.
**Last reviewed**: 2026-04-27

## Summary

Sprint 2 ticket 05 ships
[`infra/ansible/`](../../infra/ansible/) which configures the Phase
0–2 single-VPS control plane. This runbook covers two flavours of
operational use:

1. **Routine deploy** — bumping the `kubinate-api` image tag and
   re-applying the playbook.
2. **Rollback** — reverting to the previous tag when the new one
   misbehaves.

Both flows assume the playbook is idempotent on a healthy host (DoD
row 2 of ticket 05). If a "no-op" re-apply isn't no-op, treat the
host as suspect first — see "When the playbook drifts" below.

## Symptoms

- Routine deploy: a new `kubinate-api` tag was merged and the
  on-call is paged to ship it.
- Rollback: post-deploy `/readyz` is 503; or customer reports
  500-class errors that started right after the deploy.

## Severity rubric

- **SEV-2**: Paying tenants impacted *now*. Roll back first; debug
  later.
- **SEV-3**: Free-tier or pre-launch tenants only. Investigate then
  roll back if needed.

## Immediate actions (first 60 seconds)

1. Open the operator workstation that has the encrypted Ansible
   vault password ready.

2. Check the live tag:
   ```bash
   ssh kubinate@cp.kubinate.com 'docker compose -f /opt/kubinate/compose/docker-compose.yml ps --format json' \
     | jq -r '.[] | select(.Service == "kubinate-api").Image'
   ```

3. **Decide deploy vs rollback**:

   - **Deploy a new tag**:
     ```bash
     cd infra/ansible
     # Edit the tag in group_vars/all/main.yml, then:
     ansible-playbook -i inventory.ini site.yml \
       --tags kubinate_api --ask-vault-pass
     ```

     The role's final task is a 12 × 5s `/healthz` retry loop, so a
     failed pull surfaces inside the playbook run.

   - **Roll back to the previous tag**:
     ```bash
     git revert <deploy-commit>     # in infra/ansible
     ansible-playbook -i inventory.ini site.yml \
       --tags kubinate_api --ask-vault-pass
     ```

     Same retry loop. Verify `/readyz` is 200 from the operator's
     workstation through the Cloudflare Tunnel before declaring
     done.

## Diagnosis

If the playbook reports the api role as **changed** but `/healthz` is
still failing after the retry loop:

```bash
# 1. Container status
ssh kubinate@cp.kubinate.com 'docker compose -f /opt/kubinate/compose/docker-compose.yml ps'

# 2. API logs — last 200 lines
ssh kubinate@cp.kubinate.com 'docker compose -f /opt/kubinate/compose/docker-compose.yml logs --tail 200 kubinate-api'

# 3. Migration history — a failed migration leaves the api crash-looping
ssh kubinate@cp.kubinate.com 'docker compose -f /opt/kubinate/compose/docker-compose.yml exec -T postgres \
  psql -U kubinate -d kubinate -c "SELECT version, description, success FROM _sqlx_migrations ORDER BY installed_on DESC LIMIT 5;"'
```

## Mitigations

Ordered by preference.

1. **Roll back to the previous tag** (above). Smallest blast radius;
   migrations that ran on the new tag stay applied (sqlx migrations
   are forward-only — see ADR-0005), but that's normally fine because
   they're additive.

2. **Cordon Cloudflare Tunnel** if the api is healthy but spamming
   noisy errors:

   ```bash
   ssh kubinate@cp.kubinate.com 'sudo systemctl stop cloudflared'
   ```

   Returns 5xx to upstream callers; useful for quiet investigation
   of background-job behaviour. Restart with `systemctl start`.

3. **Drop to maintenance mode** by re-rendering the api env file with
   `KUBINATE__MAINTENANCE=1` (Sprint 3 follow-up — not yet wired).

## When the playbook drifts

A re-run of `ansible-playbook ... site.yml` reporting `changed > 0` on
a host that was healthy before is a *signal*, not the bug. The
common causes:

- **Container drift** — a manual `docker run` or `docker stop` left
  the stack out of sync with the rendered compose file. The role
  brings it back; this is fine.
- **Image tag drift** — the playbook's `pull: always` brings down a
  newer point-release of postgres / temporal. Pin tightly in
  `group_vars/all/main.yml` if this is unwanted.
- **Vault rotated** — handlers restart only the consuming containers.
  Expected.

If the cause is **none of the above**, snapshot the host (state on
disk + container ps + recent logs) into the incident channel before
re-applying — the divergence itself is forensic evidence.

## Recovery / rollback

- `/readyz` is 200 for 5 consecutive checks from outside the VPS.
- Cluster status pages render normally for two random tenants.
- `audit_log_verify` for the operator's own org returns no broken
  rows.

## Communications

- SEV-2: status-page note within 10 minutes of deciding to roll
  back. Update again when recovered.
- SEV-3: customer-facing comm via the support channel only if a
  paying tenant explicitly asked.

## Related

- [`hetzner-5xx-surge.md`](./hetzner-5xx-surge.md) — when the
  symptom looks like a deploy issue but is actually upstream Hetzner.
- [`provisioning-workflow-stuck.md`](./provisioning-workflow-stuck.md)
  — when in-flight provisioning workflows are wedged across a deploy.
- ADR-0004: Deployment topology (single-VPS rationale + Phase 3
  migration plan).
- ADR-0005: Database (sqlx migrations are forward-only).
- `infra/ansible/README.md`.
