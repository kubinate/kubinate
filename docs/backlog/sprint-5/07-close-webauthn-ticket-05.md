# Close out Sprint 4 ticket 05's last two rows

**Labels**: `area/identity`, `area/security`, `sprint-5`
**Epic**: ADR-0009 §MFA
**Size**: 3 (high-confidence; both pieces follow patterns the
parent ticket established and #5/#6/#7 PRs validated)

## Context

Sprint 4 ticket 05 shipped 90% of the WebAuthn-for-Owners
work — the WebAuthn ceremony layer, the `OwnerActor`
extractor, the partial-session decision in the OAuth
callback, the audit-chain entries for every passkey lifecycle
event, and the first Owner/Admin route migration (`DELETE
/v1/clusters/:id`). Two open rows remain:

1. **`requires_mfa` auto-flip on Owner/Admin promotion /
   demotion.** Today `users.mfa_enrolled` is the gate
   trigger, and it flips when a passkey is registered or
   the last live one is revoked. The parent ticket also
   committed to a `requires_mfa` flag that flips on
   role mutation so a user promoted to Owner without a
   registered passkey gets a clear "you need to enrol"
   prompt rather than silently bypassing the gate. Today
   that prompt is implicit (the gate only fires for users
   who already have a passkey).
2. **Remaining Owner/Admin route migrations.** Sprint 4
   #7 migrated `destroy` as the proof-of-concept; the
   remaining high-blast-radius routes (`workers` scaling,
   `provision`, `addons` install, `billing` checkout)
   still use the bare `Actor` extractor, so the MFA gate
   doesn't fire on them.

This ticket closes both. It also lands the threat-model v2
row for the WebAuthn boundary that the parent ticket
explicitly deferred ("Threat-model v2 captures the new
boundary: session token ↔ WebAuthn challenge").

## Acceptance criteria

- **Given** an Owner promotion path (existing
  `MembershipService::promote` or sibling), **when** the
  promotion commits, **then** the same transaction sets
  `users.requires_mfa = TRUE` for the affected user. A
  demotion in the same transaction shape sets it back to
  `FALSE` if the user holds no other Owner/Admin
  membership across any organization.
- **Given** an Owner who has not yet registered a passkey,
  **when** the OAuth callback runs, **then** the partial-
  session decision still issues a partial session
  (`mfa_satisfied = false`) AND the SPA's session-status
  endpoint surfaces a clear "you need to enrol a passkey"
  signal so the dashboard can route to
  `/app/settings/security` rather than infinitely showing
  the assertion challenge.
- **Given** the `workers`, `provision`, `addons`, and
  `billing` Owner/Admin routes, **when** a user with a
  partial session calls them, **then** the response is
  HTTP 401 with Problem Details `code = mfa_required`.
- **Given** a Member-role user, **when** they call any of
  those four routes, **then** the response is HTTP 403
  (role gate, same as `destroy`).
- **Given** Sprint 4 #6's audit hooks are in place, **when**
  an Owner is promoted, **then** the `memberships.role`
  change appears in the per-tenant audit chain via the
  existing column-aware trigger from Sprint 3 ticket 06
  (no new audit code needed; the `requires_mfa` flip is a
  side-effect, not a separate event).

## Implementation notes

- **`requires_mfa` column.** The parent ticket called for a
  separate `requires_mfa` column on `users`; today the
  effective gate is `mfa_enrolled` AND `EXISTS (Owner/Admin
  membership)`. Adding `requires_mfa` lets us decouple
  "this user must MFA" from "this user has a passkey to MFA
  with" — important for the "promoted to Owner with no
  passkey" UX. New migration adds the column with
  `DEFAULT FALSE`, then a backfill query sets it TRUE for
  every user with an active Owner/Admin membership at
  migration time.
- **Trigger or service-layer flip.** Two viable shapes:
  - (A) Postgres trigger on `memberships` that updates
    `users.requires_mfa` on INSERT / UPDATE / soft-delete.
    Atomicity is automatic; the application layer doesn't
    need to know.
  - (B) Service-layer handler in
    `crates/identity/src/service.rs` that updates both
    rows in the same transaction.
  Pick (A) — the trigger keeps the invariant guaranteed
  even if a future code path (CLI tool, direct SQL,
  whatever) writes to memberships without going through
  the service. The service-layer path is too easy to
  bypass.
- **Partial-session decision update.** Today the OAuth
  callback's `user_requires_partial_session` helper checks
  `users.mfa_enrolled AND EXISTS (Owner/Admin membership)`.
  After this ticket the helper checks
  `users.requires_mfa AND users.mfa_enrolled`. The
  decoupling is what unlocks the "enrol now" UX path —
  `requires_mfa = TRUE AND mfa_enrolled = FALSE` is the
  state that means "you need to enrol", and the SPA can
  detect it.
- **SPA hook.** Add a field to the existing `/v1/me`
  response: `mfa_state: 'not_required' | 'enrolled' |
  'must_enrol' | 'must_assert'`. The four states cover the
  matrix:
  | requires_mfa | mfa_enrolled | session.mfa_satisfied | mfa_state |
  |---|---|---|---|
  | F | * | * | not_required |
  | T | F | * | must_enrol |
  | T | T | F | must_assert |
  | T | T | T | enrolled |
- **Route migrations.** Each is a one-line extractor swap
  from `actor: Actor` to `owner: OwnerActor`, mirroring
  Sprint 4 #7. Per CONTRIBUTING.md, each route's PR wants
  per-route security review — bundle the four routes into
  one PR with the security-maintainer reviewer cc'd, OR
  split into four PRs to make per-route review crisp.
  Default to the bundle since the pattern is identical
  across all four; reviewers can request a split.

## DoD

- [ ] Migration adds `users.requires_mfa BOOLEAN NOT NULL
      DEFAULT FALSE`; backfill query populates TRUE for
      existing Owner/Admin users.
- [ ] Postgres trigger on `memberships` keeps
      `users.requires_mfa` in sync with the role-aware
      "is this user Owner/Admin somewhere" predicate.
- [ ] `session::user_requires_partial_session` updated to
      read `requires_mfa` instead of inlining the
      Owner/Admin existence subquery.
- [ ] `/v1/me` returns the four-state `mfa_state` field;
      Vitest covers all four state transitions.
- [ ] `crates/api/src/clusters.rs::workers` migrated to
      `OwnerActor`.
- [ ] `crates/api/src/clusters.rs::create` (provision)
      migrated to `OwnerActor`.
- [ ] `crates/api/src/clusters.rs::addons_install`
      migrated to `OwnerActor`.
- [ ] `crates/api/src/billing.rs::checkout` migrated to
      `OwnerActor`.
- [ ] `requires_mfa` flag flip is audit-covered (the
      existing column-aware trigger from Sprint 3 ticket
      06 picks it up automatically; verify with a test).
- [ ] **Threat-model v2 row** for the WebAuthn boundary —
      the new Flow 6 STRIDE table. The trigger ("post-Vault,
      post-observability-proxy") fires at sprint-close;
      this ticket's contribution is the Flow 6 content
      (session ↔ challenge ↔ assertion ↔ promoted session).

## What this ticket deliberately does **not** do

- **TOTP fallback.** Same exclusion as Sprint 4 ticket 05.
  Recovery codes are the lost-device path; ADR-0009 names
  WebAuthn as the only MFA technology in scope.
- **SCIM-driven role propagation.** Phase 4+ enterprise
  scope.
- **Authenticator attestation policies.** First ship
  accepts any attestation; tightening is a follow-up
  ticket if a customer needs FIPS 140-3.
- **Voluntary MFA enforcement for Member-role users.** The
  parent ticket is explicit that voluntary stays
  voluntary. A separate ticket can flip the default once
  we have telemetry on adoption.

## Per-route security review notes (for the route-migration PRs)

When the four-route migration PR opens, the reviewer needs
to confirm:

- Each route's existing handler reads `actor.user_id` and
  `actor.organization_id`; the swap to `owner.inner.user_id`
  / `owner.inner.organization_id` preserves the same access.
- No route relies on `Actor`'s "no MFA gate" behavior — i.e.
  there's no legitimate flow that calls these routes with a
  partial session intentionally.
- The `OwnerActor` extractor's role-gate query (`role IN
  ('owner', 'admin')`) excludes Developer, which is the
  expected outcome — Developers can read but not mutate.
  Confirm this matches each handler's semantic intent.

## References

- [`docs/backlog/sprint-4/05-webauthn-for-owners.md`](../sprint-4/05-webauthn-for-owners.md)
  — Sprint 4 parent ticket; the deferred rows of its DoD
  are what this ticket closes.
- [ADR-0009](../../adr/0009-authentication.md) — auth
  decision; this ticket implements the §MFA section's
  Phase 3 enforcement commitment.
- [ADR-0013](../../adr/0013-webauthn-device-lifecycle.md)
  — device-lifecycle decisions accepted in Sprint 4.
- [`crates/api/src/actor.rs`](../../../crates/api/src/actor.rs)
  — `OwnerActor` extractor (now in-tree user since Sprint
  4 #7).
- [`crates/identity/src/session.rs`](../../../crates/identity/src/session.rs)
  — `user_requires_partial_session` helper this ticket
  reshapes.
