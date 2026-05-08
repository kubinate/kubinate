# Runbook — Tenant Hetzner-token compromise / rotation

**Severity**: SEV-1 (suspected compromise of a paying tenant's token) /
            SEV-2 (free-tier tenant, customer-reported leak) /
            SEV-3 (proactive rotation, no compromise indicators)
**Alert source(s)**:
- Customer support ticket tagged `token-leak`.
- Anomalous-Hetzner-use detector (TBD per threat-model Flow 2 §Open gaps).
- Internal audit-log review flagging unexpected `hetzner_credential.created`
  events.
**Owner**: Security on-call / Identity team.
**Last reviewed**: 2026-04-26

## Summary

A tenant's Hetzner API token, which Kubinate stores encrypted in our
`secrets` table (ADR-0007, threat-model Flow 2), is suspected leaked
or the customer has asked us to rotate it. The token grants full
control of the customer's Hetzner project — the attacker can spin up
arbitrary VPS and run up the customer's bill until the token is
invalidated **at Hetzner's side**.

This runbook covers, in order: **scope confirmation**, **revocation +
ciphertext zero-out** in our database, and the **customer-comms
template**. The 60-second test: a paged engineer should be able to
get all three within one screenful below.

The runbook **only covers Kubinate-side handling**. Hetzner is the
authoritative revocation surface — we tell the customer to delete and
re-issue the token in their Hetzner Cloud Console; our DB cleanup is
defence-in-depth for the time window between leak and Hetzner-side
revocation.

## Symptoms

- Customer reports the token may have been pasted in a public place
  (chat log, screenshot, public repo).
- Kubinate audit log shows `hetzner_credential.created` events from an
  unexpected actor or IP for that tenant.
- Hetzner billing alert from the customer for unexpected spend.

## Severity rubric

- **SEV-1**: Paying tenant, evidence of active misuse (Hetzner usage
  spike or audit-log signal). Page the security on-call immediately.
- **SEV-2**: Free-tier tenant or customer-reported leak with no usage
  signal yet. Same actions, longer comms window.
- **SEV-3**: Proactive rotation, no compromise indicators. Schedule
  the rotation rather than executing it under pressure.

## Immediate actions (first 60 seconds)

### 1. Confirm scope

```bash
# Replace placeholders. organization_id is in the support ticket; if
# unknown, search by customer email:
psql "$KUBINATE_DATABASE_URL" -c "
  SELECT m.organization_id
  FROM memberships m
  JOIN users u ON u.id = m.user_id
  WHERE u.email = '<customer-email>'
    AND m.deleted_at IS NULL;
"

# List that tenant's live credentials and any recent uses. The audit
# rows here come from the trigger added in Sprint 1 ticket 09 and the
# kubeconfig retrievals from ticket 04 (`audit_log_append_explicit`).
psql "$KUBINATE_DATABASE_URL" -c "
  SELECT id, alias, created_at, updated_at
  FROM hetzner_credentials
  WHERE organization_id = '<org_uuid>' AND deleted_at IS NULL;
"

psql "$KUBINATE_DATABASE_URL" -c "
  SELECT created_at, action, resource_id, actor_user_id, request_id, ip_address
  FROM audit_log_entries
  WHERE organization_id = '<org_uuid>'
    AND resource_type IN ('hetzner_credentials', 'cluster')
    AND created_at > now() - interval '7 days'
  ORDER BY created_at DESC
  LIMIT 100;
"
```

If usage looks normal and the rotation is purely proactive, drop to
SEV-3 and schedule the change.

### 2. Revoke + zero the ciphertext

The application path is preferred — it routes through `SecretStore`
which preserves the audit chain. The SQL fallback exists for cases
where the application is unavailable.

```bash
# Application path. `kubinate-ops` is the operator CLI that calls
# `HetznerCredentialService::delete` with an explicit AuditContext
# tagged "operator/credential-rotation".
kubinate-ops hetzner-credential revoke \
  --organization-id "<org_uuid>" \
  --credential-id   "<cred_uuid>" \
  --reason          "operator-revoke: leak-suspect $(date -Iseconds)"
```

```sql
-- SQL fallback. Run as the privileged audit-bypass role; record the
-- session role in the incident channel before and after.
BEGIN;
  -- Zero the ciphertext + wrapped DEK so a database snapshot taken
  -- after this point cannot recover the plaintext, even if the KEK
  -- leaks separately.
  UPDATE secrets
  SET ciphertext  = ''::bytea,
      wrapped_dek = ''::bytea
  WHERE id IN (
    SELECT secret_id FROM hetzner_credentials
    WHERE id = '<cred_uuid>' AND organization_id = '<org_uuid>'
  );

  -- Soft-delete the credential row so the destroy workflow refuses
  -- to use it for any new run, but the audit chain stays intact.
  UPDATE hetzner_credentials
  SET deleted_at = now(), updated_at = now(), version = version + 1
  WHERE id = '<cred_uuid>' AND organization_id = '<org_uuid>';

  -- Append a hash-chained audit row covering this manual action.
  SELECT audit_log_append_explicit(
    '<org_uuid>',
    'hetzner_credential.revoked',
    'hetzner_credentials',
    '<cred_uuid>',
    'allowed',
    '{"runbook": "credential-rotation", "reason": "leak-suspect"}'::jsonb
  );
COMMIT;
```

After the revoke, instruct the customer to **delete the token in
Hetzner Cloud Console** — that's the only action that prevents the
leaked value from being used elsewhere. Our DB cleanup just stops
*us* from accidentally using it on their behalf.

### 3. Customer-comms template

Paste into the customer-facing channel:

```
Hi <name>,

We have invalidated the Hetzner API token <alias> on our side at
<UTC timestamp> as a precaution. To complete the rotation:

  1. Open https://console.hetzner.cloud/projects → Security →
     API Tokens, and **delete** the existing token.
  2. Generate a new token with the same scope (read+write).
  3. Add it to Kubinate: Settings → Integrations → Add Hetzner
     credential.

Once the new token is in place, your existing clusters will continue
to operate. New provisioning runs will automatically pick up the new
token. We will run our standard audit-log review covering the last
24 hours and follow up if anything looks off; please reply to this
thread with anything unexpected on your Hetzner billing in the
meantime.
```

## Diagnosis (deeper investigation)

If usage signal suggests active abuse:

```bash
# Recent provisioning workflows under that org — anything we didn't expect?
psql "$KUBINATE_DATABASE_URL" -c "
  SELECT id, cluster_id, current_step, started_at, error_reason
  FROM provisioning_workflows
  WHERE organization_id = '<org_uuid>'
    AND started_at > now() - interval '24 hours'
  ORDER BY started_at DESC;
"

# Audit hash-chain integrity — if the chain breaks for this tenant,
# treat as forensic-evidence-only and engage security lead.
psql "$KUBINATE_DATABASE_URL" -c "
  SELECT * FROM audit_log_verify('<org_uuid>');
"
```

## Mitigations

Ordered by preference.

1. **Application revoke** (step 2 above). Smallest blast radius —
   leaves the rest of the tenant's Kubinate setup untouched.
2. **SQL revoke** when the API is down. Same effect, must be logged
   manually in the incident channel.
3. **Org-wide token freeze**: if multiple tokens for one tenant look
   compromised, mark every credential `deleted_at = now()` for that
   org. Operator must coordinate with the customer first — every
   in-flight provisioning will fail until they re-add a token.
4. **KEK rotation** — out of scope here. Triggered by the Phase 3
   Vault migration runbook, not this one.

## Recovery / rollback

- Customer reports the new token is in place and cluster operations
  resume.
- `audit_log_verify(<org_uuid>)` returns zero broken rows.
- The compromised token is no longer accepted by Hetzner (verify by
  attempting a `GET /v1/locations` with it from the operator host
  and expecting `401`).

## Communications

- **SEV-1 / SEV-2**: post on the Kubinate status page only if the
  customer has explicitly authorised disclosure. Default is to
  handle privately.
- **Internal**: incident channel `#incident-YYYYMMDD-cred-rotation`
  with the audit-log excerpt and the revocation timestamp pinned.
- **Postmortem** required for SEV-1 or any case where Hetzner
  billing showed unexpected spend before revoke.

## Related

- [provisioning-workflow-stuck.md](./provisioning-workflow-stuck.md)
  — for the post-revoke recovery side, when in-flight workflows fail
  because the credential has gone away.
- [hetzner-5xx-surge.md](./hetzner-5xx-surge.md) — if the symptom
  presents as a 5xx surge rather than a leak.
- ADR-0007: Secret management (envelope encryption / KEK lifecycle).
- Threat model Flow 2: User supplies a Hetzner API token.
- Sprint 1 ticket 09 (audit chain) and ticket 04 (kubeconfig
  retrieval audit). Both feed the audit queries above.
