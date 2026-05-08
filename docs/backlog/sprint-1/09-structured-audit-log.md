# Persist audit log entries for token and cluster mutations

**Labels**: `area/identity`, `area/platform`, `area/security`, `sprint-1`
**Size**: 2

## Context

Threat model requires tamper-evident audit for tier-1 actions.
Full UI for audit is Phase-3 scope; Sprint 1 just persists the entries
with hash chaining.

## Acceptance criteria

- **Given** a write on `hetzner_credentials`, `clusters`, or
  `sessions`, **then** an `audit_log_entries` row is appended with
  actor, resource, decision, request_id, IP, UA, and a hash chained
  to the previous entry for the tenant.
- **Given** a tampered row (simulated), **then** a verification
  function detects the break in the chain.

## DoD
- [ ] Verification function + unit test
- [ ] Write is same-transaction with the triggering mutation
- [ ] Bypass role does not skip the audit append
