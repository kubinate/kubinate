# Runbook: tenant Hetzner-token compromise / rotation

**Labels**: `area/sre`, `area/runbooks`, `area/security`, `sprint-2`
**Size**: 1

## Context

We accept user Hetzner tokens (Sprint 1 ticket 01). The threat model
expects, eventually, an incident where one is leaked — accidentally
posted, exfiltrated by a tenant compromise, or rotated proactively by
a customer. We need a runbook on file before the first such event,
not after.

## Acceptance criteria

- **Given** an engineer paged about a leaked tenant token, **when**
  they open `docs/runbooks/credential-rotation.md`, **then** within
  60 seconds they can:
  1. Confirm scope — which tenant, which credential id, has the
     token been used since the suspected leak.
  2. Find the SQL or `kubinate-ops` command to mark the credential
     `revoked` and zero its ciphertext.
  3. Find the customer-comms template snippet.
- Runbook follows the template.

## DoD

- [ ] Runbook reviewed by a second engineer.
- [ ] Linked from `docs/security/threat-model.md` Flow 2 (Hetzner
      token handling).
- [ ] Linked from the new SRE index in `docs/runbooks/README.md` (if
      that index doesn't exist, ship the smallest possible version of
      it as part of this ticket).
