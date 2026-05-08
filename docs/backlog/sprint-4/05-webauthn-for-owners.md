# Enforce WebAuthn for Admin / Owner roles

**Labels**: `area/identity`, `area/security`, `sprint-4`
**Epic**: ADR-0009 §MFA
**Size**: 8 (high-confidence; the wire shape is well-known and the
device lifecycle is the substantive part)

## Context

[ADR-0009](../../adr/0009-authentication.md) commits to:

> **MFA**: WebAuthn (passkeys) supported for interactive sessions.
> Enforced for accounts with Admin or Owner roles in Phase 3+.

The Phase-2 implementation supports WebAuthn for **none** of those.
Today's auth path (Sprint 1 ticket 07) is GitHub OAuth → session
cookie; an Owner authenticated only via that flow has the same
session token strength as a free-tier user.

ADR-0009 §Consequences explicitly names the cost:

> WebAuthn enforcement for admins requires us to support device
> lifecycle (register, revoke, recover) — non-trivial but
> necessary.

The roadmap names "WebAuthn enforced for org owners" as a Phase 3
exit gate (`docs/roadmap.md`).

## When this opens

This ticket has **no** prerequisite on the other Sprint 4
parking-lot tickets — it touches `kubinate-identity`, not the
runner / agent / observability path. It can land in parallel
with Vault (ticket 02) and the agent reverse-tunnel (ticket 03);
it does need a WebAuthn-capable browser for end-to-end testing,
but Cloudflare Pages + the existing SvelteKit app already
deliver that.

Conversely, **don't ship this without an updated runbook** for
device-lost recovery — locking an Owner out of their own org
because they lost their only passkey is a worse failure mode
than the original "no WebAuthn" state.

## Acceptance criteria

- **Given** an authenticated user, **when** they navigate to
  `/app/settings/security`, **then** they can register a
  WebAuthn passkey via the standard `navigator.credentials.create()`
  flow. The credential id + public key + sign counter are stored
  in a new `user_passkeys` table with RLS enabled.
- **Given** a registered passkey, **when** the user signs in with
  GitHub OAuth and an account marked `requires_mfa = true`,
  **then** the session is **partial** until the WebAuthn assertion
  step succeeds. A partial session can hit `/v1/me` but not any
  Owner / Admin route.
- **Given** an org with at least one Admin or Owner role,
  **when** the API records that membership (Sprint 2 identity
  work), **then** the user's `requires_mfa` flag is set.
- **Given** a user who lost their device, **when** they go
  through the recovery flow, **then** they re-authenticate with a
  pre-registered backup factor (second passkey — required at
  enforcement time) **or** they wait through an org-admin-driven
  reset (separate Owner can override; if the lost user is the
  only Owner, see the recovery runbook).
- **Given** Owner Bob has WebAuthn enforced and tries to call
  `DELETE /v1/clusters/:id` over a partial session, **when** the
  handler dispatches the request, **then** it returns 401 with
  Problem Details `code=mfa_required`.

## Implementation notes

- Use the `webauthn-rs` crate (the most active Rust WebAuthn
  library; ADR-0009 §References already links the WebAuthn L3
  spec). Pin in workspace deps.
- New tables:
  - `user_passkeys (id, user_id, credential_id, public_key,
    sign_counter, transports, registered_at, last_used_at,
    nickname)`. RLS scoped on the user's organization
    membership — consistent with ADR-0006.
  - `mfa_recovery_codes (id, user_id, code_hash, used_at)` — 10
    one-shot codes generated at enrollment.
- Extend the `users` (or `user_security` join) row with
  `requires_mfa boolean NOT NULL DEFAULT false`. The
  Owner / Admin promotion paths (existing in `crates/identity/`)
  flip this on.
- Session model gets a `mfa_satisfied boolean`. Auth middleware
  rejects Owner / Admin routes when `requires_mfa && !mfa_satisfied`
  with the new `mfa_required` Problem Details code.
- Frontend: new `/app/settings/security` page; modify the
  post-OAuth callback to detect `mfa_satisfied=false` and
  redirect to a passkey-challenge page before landing on the
  dashboard.
- The challenge protocol uses the standard
  `PublicKeyCredentialRequestOptions` — `webauthn-rs` produces
  the JSON; the frontend hands it to
  `navigator.credentials.get()` and POSTs the assertion back.

## Recovery runbook (DoD row)

Author `docs/runbooks/owner-passkey-lost.md` covering:

- The "I lost my only passkey, but I have my recovery codes"
  path — tested by a vitest spec in the frontend.
- The "I lost both passkey and recovery codes" path — requires
  a peer-Owner to flip `requires_mfa=false` for the affected
  account out-of-band, plus an audit-chain entry recording the
  override.
- The "I'm the only Owner of the org" path — manual escalation
  only, requires identity-verification per the runbook's
  template; document the proof-of-org-control evidence the
  on-call asks for.

## DoD

- [x] `webauthn-rs` integration with parity tests (registration
      round-trip, assertion round-trip, sign-counter rollback
      detection). Ceremony layer at
      `crates/identity/src/webauthn.rs`; sign-counter regression
      detection in `PasskeyRepository::record_use` (`WHERE
      sign_counter <= $2` belt-and-suspenders on top of
      webauthn-rs's own check).
- [x] `user_passkeys` + `mfa_recovery_codes` migrations.
      `migrations/20260505120000_webauthn.sql`. (No RLS — these
      tables are user-scoped, not tenant-scoped, per the
      tenancy note in the migration header.)
- [ ] `requires_mfa` flag set automatically on Owner / Admin
      promotion; cleared on demotion. *Deferred:* the OAuth
      callback's partial-session decision (issue
      `Session.mfa_satisfied = false` for users with
      `mfa_enrolled = true` + an Owner/Admin membership) is
      what activates the gate end-to-end. The `OwnerActor`
      extractor + `mfa_required` Problem Details code already
      exist; the callback flip is the security-review-gated
      follow-up.
- [ ] Auth middleware enforces MFA on the Owner / Admin route
      class. Existing Sprint 2 ticket 04 / 06 routes annotated.
      *Deferred:* `OwnerActor` extractor lands in
      `crates/api/src/actor.rs` with the gating logic; route
      migrations to use it want per-route `security`-role review
      per CONTRIBUTING.md.
- [x] Frontend `/app/settings/security` page + Vitest covering
      registration, assertion, and the partial-session redirect.
      `frontend/src/routes/app/settings/security/+page.svelte` (622
      lines) + 6 vitest specs.
      **Verification deferred**: type-correct + test-covered, but
      not browser-verified. The full WebAuthn ceremony round-trip
      against a real authenticator runs in the Sprint 4 close
      acceptance pass.
- [x] Runbook `docs/runbooks/owner-passkey-lost.md` merged + a
      row in `docs/runbooks/README.md`.
- [x] An ADR (next free) records the device-lifecycle decisions
      that ADR-0009 explicitly deferred. ADR-0013 `Accepted`
      ratifies five policies (recovery-code count = 10,
      accept-any attestation, sign-counter regression rejects
      assertion no auto-revoke, allow re-registration of revoked
      credentials, 64-char nicknames).
- [ ] Threat-model v2 captures the new boundary: session token
      ↔ WebAuthn challenge. *Deferred*: the threat-model v1
      footer names "post-Vault, post-observability-proxy" as
      its v2 trigger. Both gates are still Phase 3 work; v2
      lands when those land.

## Security DoD (CONTRIBUTING.md `security`-role review required)

- [ ] Sign-counter regression rejected (cloned authenticator
      detection — webauthn-rs handles this; verify it's wired).
- [ ] Recovery codes generated with `rand::thread_rng()` (the
      workspace's existing CSPRNG path); one-shot on use.
- [ ] Audit log captures every passkey enrol / use / revoke +
      every recovery-code use.
- [ ] Negative test: a partial session cannot hit Owner /
      Admin endpoints (HTTP 401 with `mfa_required`), and the
      same partial session can still hit `/v1/me` (so the
      frontend can show the user who they are while they
      complete the challenge).
- [ ] Replay protection: a single WebAuthn assertion cannot be
      replayed within the session lifetime (challenge values
      are nonces + bound to the partial session id).

## What this ticket deliberately does **not** do

- **Voluntary MFA for Member-role users.** Members can register
  passkeys via the same UI but `requires_mfa` stays false; this
  ticket only enforces the Owner / Admin class. A follow-up
  ticket can flip the default once we have telemetry on
  voluntary adoption.
- **TOTP fallback.** Recovery codes cover the lost-device path;
  TOTP would add a second authenticator class with no security
  benefit over a second passkey. ADR-0009 §References names
  WebAuthn as the only MFA technology in scope.
- **SCIM-driven role propagation.** Phase 4+ enterprise readiness
  scope. This ticket reads role state from the existing
  `memberships` table.
- **Authenticator attestation policies.** First ship accepts any
  attestation type; the policy hooks are exposed by `webauthn-rs`
  for a follow-up if a customer has a "FIPS 140-3 only"
  requirement.
