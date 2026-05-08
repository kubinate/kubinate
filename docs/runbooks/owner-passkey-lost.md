# Runbook — Owner / Admin lost their passkey

**Severity**: SEV-3 (single-user lockout, business-as-usual recovery)
            → SEV-2 if the affected user is the **only** Owner of
            their org and the org has paying alpha-tier infrastructure
            in production.
**Alert source(s)**:
- Customer support ticket tagged `mfa-lockout`.
- Internal Slack message from the affected user via a peer Owner
  / Admin.
- (Phase 4+) `KubinateMfaRedeemFailureRate` Prometheus alert
  fires when redeem attempts spike — see Follow-ups.
**Owner**: Identity & Access team (rotates with on-call SRE for
            after-hours).
**Last reviewed**: 2026-05-05

## Summary

Sprint 4 ticket 05 ships WebAuthn enforcement for Owner and Admin
roles ([ADR-0009](../adr/0009-authentication.md) §MFA;
[ADR-0013](../adr/0013-webauthn-device-lifecycle.md) ratifies the
device-lifecycle decisions). When an Owner / Admin loses their
passkey, three loss profiles cover every path back to a working
session:

1. **They have a recovery code.** Self-service via the
   `/v1/auth/recovery-codes/redeem` endpoint. No operator
   intervention.
2. **They lost both passkey and recovery codes, but a peer
   Owner / Admin can vouch.** Operator-mediated reset of
   `users.mfa_enrolled` to `false` for the affected account, plus
   an audit-chain entry recording the override.
3. **They are the sole Owner of their org and have lost both.**
   Manual identity-verification escalation per the playbook
   below; the on-call answers a small set of evidence questions
   before a `users.mfa_enrolled` reset is approved.

The threat model: an attacker who has compromised an account
can also claim "I lost my passkey" to attempt evading MFA. **The
identity-verification step in path 3 is the gate against that
attack vector.** Skipping it converts MFA into security theatre.

## Symptoms

- User reports "I see an `mfa_required` Problem Details response"
  on actions that previously worked. The 401 + body
  `{"code": "mfa_required", ...}` is the gate firing.
- User reports their browser doesn't show the registered
  authenticator (lost / stolen device, replaced laptop without
  syncing platform passkey, etc).
- The user has not yet redeemed any recovery codes for this loss
  event (`/v1/auth/recovery-codes/redeem` returning 200 means
  they're already past this runbook).

## Severity rubric

- **SEV-3**: a peer Owner / Admin in the same org can perform
  path 2. The affected user resumes work within ~30 minutes.
- **SEV-2**: the affected user is the **only** Owner of an org
  with active production infrastructure (running clusters with
  customer load). Path 3 applies; the on-call's response time
  matters because the org is operating without an unblocked
  Owner.
- **SEV-1**: never. A lost passkey is not a service incident; if
  the lockout coincides with an actual outage that requires
  Owner action, the parent incident's severity governs.

## Immediate actions (first 5 minutes)

1. Verify the user. **Do not** start the recovery flow over a
   support channel that hasn't authenticated them — every step
   below assumes the request is from the legitimate owner of the
   account.
2. Confirm which loss profile applies. The discriminator is "do
   you have any of the recovery codes you generated when you
   enrolled?"
3. Check whether the user is the sole Owner of their org:
   ```sql
   SELECT count(*) FROM memberships
   WHERE organization_id = '<org-id>' AND role = 'owner'
     AND deleted_at IS NULL;
   ```
   If 1 → path 3 (sole Owner). If > 1 → path 2 (peer-vouched) is
   available even if they pick path 3 anyway.

## Path 1 — recovery code redeem (self-service)

The user types one of the codes from the batch they downloaded at
enrollment into the security-settings page (or, for the assertion
flow, into the partial-session login screen).

- **Endpoint** the page hits:
  `POST /v1/auth/recovery-codes/redeem`
  body `{"code": "<plaintext>"}`.
- **Server flow**: SHA-256 of the canonical (uppercase,
  hyphen-stripped) form looks up
  `mfa_recovery_codes.code_hash`; the row is consumed atomically
  via `UPDATE … WHERE used_at IS NULL RETURNING id`.
  [`crates/identity/src/repository.rs::PgRecoveryCodeRepository::try_consume`](../../crates/identity/src/repository.rs)
  is the source of truth.
- **Result**: the partial session is promoted (`mfa_satisfied`
  flips to `true` via `kubinate_identity::session::mark_mfa_satisfied`),
  and the response includes `remaining` so the UI can show the
  user how many codes are left.

After a successful redeem, **strongly recommend** they regenerate
the batch (`POST /v1/auth/recovery-codes/regenerate`) — a code
they used in a hostile environment (typed into a borrowed laptop
during a runbook escalation) should not still be valid for the
remaining 9 codes' attacker.

## Path 2 — peer Owner / Admin vouches + resets `mfa_enrolled`

When the user has lost both passkey and recovery codes but
another Owner / Admin in the same org can vouch:

1. **Both** parties join the support channel. Identity-verify
   each one independently (per path 3 evidence list — even the
   vouching peer); a vouching account that's also been
   compromised would otherwise let an attacker walk the org.
2. The peer Owner / Admin **temporarily** sets
   `users.mfa_enrolled = false` for the affected user via:
   ```sql
   UPDATE users
   SET mfa_enrolled = false, version = version + 1
   WHERE id = '<user-id>';
   ```
   Run from the operator console with audit context attached
   (the `audit::AuditContext` middleware records who issued the
   override).
3. The affected user signs in via OAuth — without
   `mfa_enrolled` they get a full session immediately and can
   reach `/app/settings/security`.
4. They register a fresh passkey + regenerate recovery codes.
5. The peer Owner / Admin **flips `mfa_enrolled` back to true**
   once the affected user confirms they've enrolled. The flip
   is the matching audit entry to step 2.
6. **All three steps** (the two flips + the re-enrollment) land
   in the audit chain. Surface them in the per-org audit log
   review.

## Path 3 — sole Owner of org, lost both

The hard case. The on-call must establish the requester is the
real Owner before any reset; otherwise an attacker who has
control of the user's email + a plausible-sounding story walks
into the org.

### Evidence required

The on-call asks for **at least three** of the following before
running the override. Document each answer in the incident
record.

- The org's billing email address (matches Stripe customer
  record).
- The last four digits of the card on file (read off the Stripe
  dashboard; never quoted to the user).
- The `cluster.id` of any cluster created by this user — they
  should know it from their dashboard.
- A SSH key fingerprint registered on one of their nodes (they
  can read it off `ssh-keygen -lf ~/.ssh/id_ed25519.pub` if they
  used that key during onboarding).
- A signature with a previously-registered GPG key (if the user
  has one — most won't).
- Outbound video call confirming the user matches the
  display-name + avatar previously registered.

If the user can produce three pieces of evidence, the on-call
proceeds. If not — explicitly halt and escalate to the security
maintainer rather than proceeding under uncertainty. **A blocked
Owner is recoverable; an MFA bypass given to an attacker is
not.**

### Reset commands

```sql
-- 1. Flip the gate. Audit context attached.
UPDATE users
SET mfa_enrolled = false, version = version + 1
WHERE id = '<user-id>';

-- 2. Optional: revoke every existing passkey + recovery code so
--    the lost devices, if recovered by an attacker, cannot be
--    used. We err on the side of revoking — the user re-enrols
--    cleanly.
UPDATE user_passkeys SET revoked_at = now() WHERE user_id = '<user-id>';
DELETE FROM mfa_recovery_codes WHERE user_id = '<user-id>';
```

Both queries land in the `audit_log` per ADR-0006. The post-incident
review includes them.

### After reset

The user signs in via OAuth, lands on the `/app/settings/security`
page, registers a fresh passkey, and regenerates recovery codes.
The on-call **does not** flip `mfa_enrolled` back to `true`
manually — the next OAuth callback that observes
`user_passkeys` rows for the user re-issues a partial-MFA
session, which closes the loop naturally.

The on-call's last step: confirm a successful new-passkey
registration via the audit log:

```sql
SELECT actor_user_id, action, created_at
FROM audit_log
WHERE actor_user_id = '<user-id>'
  AND created_at > now() - interval '1 hour'
ORDER BY created_at;
```

A passkey-registration entry within the recovery window is the
"all clear" signal.

## Recovery / rollback

- The user can complete an authenticated action that was
  previously blocked by `mfa_required`.
- The audit log shows the `mfa_enrolled` flip(s) and the new
  passkey registration in the same incident timeline.
- For path 3: a follow-up email to the user (from the on-call
  rotation, not automated) confirms what was changed and asks
  them to verify no other unexpected changes appeared.

## Communications

- **Path 1**: no comms needed. The redeem endpoint already
  handled it.
- **Path 2**: brief Slack thread in the org's incident channel
  (or DM if the org doesn't have one) noting both parties
  participated. No external comms.
- **Path 3**: post-incident email to the user from the on-call
  + a copy in the security maintainer's queue. Internal: file a
  short post-mortem (≤ 1 page) within 5 business days noting
  the evidence collected, the reset performed, and any
  improvement we'd make to the evidence list.

## Follow-ups

The first time this runbook is exercised at SEV-2 or above, file
all of:

- Brute-force protection on `/v1/auth/recovery-codes/redeem`. Not
  in scope for Sprint 4 — the existing kubeconfig rate limiter
  shape (`crates/api/src/rate_limit.rs`) ports cleanly. ADR-0013
  named this as a follow-up.
- A `KubinateMfaRedeemFailureRate` Prometheus alert. Triggers
  the proactive variant of this runbook.
- A first-class operator console for path 2 + 3 — the SQL
  queries above work but a button labelled "reset MFA for this
  user (with audit reason)" would reduce typo risk.
- A self-service "transfer Owner role" flow so a sole Owner
  isn't a sole point of failure for path 3 in the first place.
  Out of scope for the WebAuthn ticket; track separately.

## Related

- [ADR-0009](../adr/0009-authentication.md) §MFA: the parent
  commitment.
- [ADR-0013](../adr/0013-webauthn-device-lifecycle.md): the
  device-lifecycle policies this runbook implements
  operationally.
- [`docs/backlog/sprint-4/05-webauthn-for-owners.md`](../backlog/sprint-4/05-webauthn-for-owners.md):
  the Sprint 4 ticket that called out this runbook as a DoD row.
- `crates/identity/src/recovery_codes.rs`: the canonicalisation
  rules a user-typed code passes through.
- `crates/api/src/auth_passkey.rs`: the redeem + regenerate
  endpoints.
