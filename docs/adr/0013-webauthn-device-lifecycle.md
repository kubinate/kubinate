# ADR-0013: WebAuthn device-lifecycle decisions

- **Status**: Accepted
- **Date**: 2026-05-05
- **Deciders**: Cluster Orchestrator team + security maintainer
- **Tags**: identity, security, webauthn
- **Relates to**: [ADR-0009](./0009-authentication.md) §MFA (the
  "WebAuthn enforced for Admin/Owner roles in Phase 3+" commitment)
  and §Consequences (the "device lifecycle is non-trivial" footnote
  this ADR closes).

## Context

[ADR-0009](./0009-authentication.md) committed to WebAuthn for Admin
and Owner role MFA in Phase 3+. Its §Consequences section
explicitly named — but did not decide — the device-lifecycle
questions:

> WebAuthn enforcement for admins requires us to support device
> lifecycle (register, revoke, recover) — non-trivial but
> necessary.

Sprint 4 ticket 05 ships the implementation:
[`crates/identity/src/webauthn.rs`](../../crates/identity/src/webauthn.rs)
(ceremony layer),
[`crates/identity/src/recovery_codes.rs`](../../crates/identity/src/recovery_codes.rs)
(recovery codes),
[`crates/identity/src/repository.rs`](../../crates/identity/src/repository.rs)
(`PasskeyRepository` + `RecoveryCodeRepository`), and the
[`auth_passkey`](../../crates/api/src/auth_passkey.rs) API surface.

The implementation made several lifecycle decisions de facto. This
ADR ratifies them as policy + names what they do **not** decide so a
future contributor doesn't accidentally widen scope without
revisiting the trade-off.

## Decision

We adopt the following five lifecycle policies. Each is stated as a
rule + the reason; the ticket and the implementation are the
sources-of-truth for the mechanism.

### 1. Recovery codes — 10 per batch, regenerated atomically

Per-user batches of **10 codes**, Crockford-Base32 alphabet (no
`I`/`L`/`O`/`U` glyphs), 10-char codes hyphen-separated halves
(`AB12C-D3E4F`). Storage is SHA-256 of the canonical (uppercase,
hyphen-stripped) form.

- **Why 10**: matches GitHub / GitLab / Auth0 defaults. 10 is
  enough that a user who hasn't rotated in 6 months still has
  unused codes after a few losses; few enough that the security
  settings page can render them without paginating.
- **Why Crockford Base32**: typability under pressure. An on-call
  engineer dictating a code in a runbook escalation path can
  pronounce every glyph unambiguously.
- **Why atomic regeneration**: any prior code — used or unused —
  becomes invalid the moment the user requests a new batch. No
  reason for an old code to stay live; the security model is
  "a code I had at time T is no longer trusted at time T+1 if
  the user said so."

### 2. Sign-counter regression → reject + force re-enroll

[`webauthn-rs`](https://crates.io/crates/webauthn-rs) detects
authenticator cloning by tracking the per-credential `sign_counter`
that the authenticator advances on every assertion. Our
[`PasskeyRepository::record_use`](../../crates/identity/src/repository.rs)
adds an additional defence-in-depth `WHERE sign_counter <= $2`
guard so a re-played assertion can't roll the counter backwards.

When a regression is detected, the assertion **fails** (the user
cannot complete that assertion). The user can:

1. Try a different registered passkey on the same authenticator.
2. Use a recovery code to satisfy MFA.
3. Re-register the affected device after re-authenticating via
   the above paths.

We do **not** automatically revoke the regressed passkey on
detection — that would let a transient bug or a one-time
counter-tracking glitch invalidate a working device. Manual revoke
via the security settings page is the supported path; an
operations follow-up adds an automatic-revoke policy if we observe
real cloning attempts.

### 3. Attestation — accept any, log the type

The first ship accepts every WebAuthn attestation type
(`packed`, `tpm`, `apple`, `none`, etc). No allow-list, no FIPS
lockdown.

- **Why accept-any**: the customer-base for Phase 3+ is heterogeneous
  (mixed authenticator brands, BYO passkey managers, platform
  authenticators on every OS). Restricting to specific attestations
  blocks legitimate users without a corresponding security gain
  for our threat model.
- **Why log the type**: a future "FIPS 140-3 only" customer
  requirement materialises as a per-org policy flag; we want the
  data to know whether existing users would be affected before we
  ship the flag.

The hooks `webauthn-rs` exposes for stricter attestation policies
(`with_attestation_ca_list`, etc) stay available — the policy
flips later without a re-design.

### 4. Re-registration of revoked passkeys — allowed

The migration's index on `user_passkeys.credential_id` is
**partial-unique on `revoked_at IS NULL`**. A user who revokes a
passkey and later wants to re-register the same authenticator can
do so; the revoked row stays for the audit story (a future per-
event audit log for passkey enrol/use/revoke), the new row is
live.

- **Why allowed**: a user re-registering their YubiKey after a
  revoke is the same authenticator binding to the same user — no
  security boundary crossed. Forbidding it would be friction
  without a corresponding gain.
- **What this is not**: the same `credential_id` cannot be live
  on **two** users at once; the partial-unique index also covers
  cross-user collisions when both rows are live.

### 5. Device naming — user-supplied, 64-char cap, no constraints on content

Users supply a nickname when registering (`"YubiKey 5C"`,
`"iPhone 15 Pro"`). The API validates non-empty + 64-char cap;
no character-class restrictions, no uniqueness constraint per
user.

- **Why 64 chars**: longer than every common authenticator name;
  shorter than the `users.display_name` cap. Keeps the security
  settings page renderable without horizontal scroll.
- **Why no uniqueness**: a user with two iPhones may legitimately
  call both `"iPhone"`; the security page can disambiguate by
  registration date. Forcing unique nicknames is friction.
- **Why no character-class restrictions**: the nickname renders
  in the security settings page (escaped at the Svelte template
  level — no HTML injection surface) and in the recovery runbook
  ("which device did you lose? the one called `<nickname>`?"). No
  wire-format depends on the bytes.

## Alternatives considered

### TOTP fallback alongside WebAuthn

Rejected. Recovery codes already cover the lost-device path. TOTP
adds a second authenticator class with the same threat profile
(shared-secret, shoulder-surfable, phishable in some flows) for no
incremental security gain. ADR-0009 §References names WebAuthn as
the only MFA technology in scope; this ADR maintains that
discipline.

### Mandatory hardware attestation (FIPS 140-3 only)

Rejected for the first ship. Customer demand is the trigger; we
have none. The hooks stay open in webauthn-rs; flipping it on is
a future per-org policy ticket if a customer materialises.

### Auto-revoke on sign-counter regression

Rejected. Real authenticators do occasionally produce regressions
without being cloned (firmware bugs, USB device-state issues, sync
glitches between platform authenticators on multiple Apple
devices). Auto-revoke would create a self-DoS path under
operationally-normal conditions. Manual revoke is the supported
path; we revisit if telemetry shows real cloning.

### Forbid re-registration of revoked passkeys

Rejected. Friction without security gain (see decision 4).

## Consequences

### We accept

- **Recovery codes are a self-help path, not a managed-recovery
  path.** The runbook `owner-passkey-lost.md` covers the
  managed-recovery escalation paths for users who've lost both
  passkey **and** recovery codes; that flow is operator-mediated
  with explicit identity verification.
- **A FIPS-only customer requirement is a future ticket.** Until
  one materialises, we accept any attestation.
- **A regressed sign-counter blocks the affected passkey + the
  user must use a different factor.** This is friction for a
  user with one passkey and lost recovery codes; the runbook
  covers it.
- **The `users.mfa_enrolled` flag is the gate that the OAuth
  callback eventually uses to decide partial vs full session
  issuance.** Sprint 4 ships the data layer; the callback flip is
  a follow-up ticket gated on `security`-role review per
  CONTRIBUTING.md.

### We assume

- **Crockford Base32 + 10 codes is sufficient one-shot entropy.**
  32¹⁰ ≈ 10¹⁵ possibilities per code, and any code is one-shot.
  Brute-force attempts are rate-limited at the recovery-redeem
  endpoint (the same Sprint 1 ticket 04 rate limiter that gates
  the kubeconfig download — currently 5/min/user; the recovery
  endpoint reuses it as a follow-up).
- **`webauthn-rs` stays maintained.** It's the most active Rust
  WebAuthn library; if it goes unmaintained, we re-spike. The
  data shape (CBOR-encoded `Passkey` blobs in
  `user_passkeys.credential`) survives a library swap because
  CBOR is the WebAuthn wire format itself.
- **Recovery codes are stored only as SHA-256.** Even with full
  DB access an attacker cannot replay codes — they have to
  brute-force the hash, which at 10¹⁵ possibilities and a
  one-shot consumption window is uneconomic.

### Second-order effects

- The `mfa_recovery_codes` table is user-scoped, not tenant-scoped.
  A user with multiple memberships uses one set of codes — same
  shape as `user_passkeys`. Documented in the migration
  (`20260505120000_webauthn.sql`).
- The OAuth callback's eventual partial-session decision (issue
  `Session.mfa_satisfied = false` for users with `mfa_enrolled`
  + an Owner/Admin membership) is the remaining route-gate flip.
  The `OwnerActor` extractor + `mfa_required` Problem Details
  code already exist; the callback is what activates them.

## Revisit trigger

This ADR re-opens automatically when **any** of the following
fire:

1. **A customer requires hardware-attestation enforcement** (FIPS
   140-3 / "this org only accepts YubiKey-class authenticators").
   Decision 3 flips to a per-org policy.
2. **Sign-counter regression telemetry shows real cloning.** If a
   pattern emerges (same `cluster_id`, multiple distinct user-
   agents, all on the same passkey), auto-revoke moves from
   "rejected" to "shipped" in decision 2.
3. **Recovery-code brute-force attempts cross a noise floor.**
   Threshold is "more than 10 redeem-attempts per user per hour
   sustained over a day"; once observed, decision 1 expands to
   include rate-limit + alert.
4. **The OAuth callback's partial-session flip lands.** Once
   route-level enforcement is on, this ADR's "the gate is wired
   but dormant" framing changes; a follow-up ADR records the
   activation.

## References

- [ADR-0009](./0009-authentication.md): the parent commitment.
- [`docs/backlog/sprint-4/05-webauthn-for-owners.md`](../backlog/sprint-4/05-webauthn-for-owners.md):
  the Sprint 4 ticket this ADR closes a row on.
- [`migrations/20260505120000_webauthn.sql`](../../migrations/20260505120000_webauthn.sql):
  schema for `user_passkeys` + `mfa_recovery_codes`.
- [`migrations/20260505120100_webauthn_ceremonies.sql`](../../migrations/20260505120100_webauthn_ceremonies.sql):
  short-lived ceremony state.
- [`crates/identity/src/recovery_codes.rs`](../../crates/identity/src/recovery_codes.rs):
  alphabet + canonicalisation rules.
- [`crates/identity/src/webauthn.rs`](../../crates/identity/src/webauthn.rs):
  ceremony facade.
- [`docs/runbooks/owner-passkey-lost.md`](../runbooks/owner-passkey-lost.md):
  the operational counterpart to this policy.
- [WebAuthn Level 3](https://www.w3.org/TR/webauthn-3/): the
  underlying spec.
