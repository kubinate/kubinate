# ADR-0009: Authentication

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: security, authentication, identity

## Context

Kubinate's user base spans:

- **Indie hackers and small teams**: want to sign up with a social
  identity (GitHub, Google) in one click.
- **Mid-sized DevOps teams**: need to use their existing identity
  provider (Google Workspace, Microsoft) and manage access centrally.
- **Enterprise customers (Phase 4)**: require SSO against their own
  IdPs (Okta, Azure AD, generic OIDC) and usually SCIM for provisioning.
- **Machine clients**: CI systems, Terraform providers, our own agent
  running in user clusters.

We must not store passwords unless we absolutely have to. Every
password store is a liability.

## Decision

- **Primary user authentication**: Kubinate is an **OIDC Relying
  Party**. Supported external IdPs at launch:
  - GitHub (OAuth 2.0 with OIDC-compatible flow)
  - Google (OIDC)
  - Microsoft (OIDC)
  - Generic OIDC (enterprise, Phase 4)
  We do **not** maintain a password database in Phases 0–4.
- **Session model**:
  - On successful IdP authentication, the server issues an opaque
    **session ID** stored as an HttpOnly, Secure, SameSite=Strict
    cookie.
  - Session records live in Postgres (`sessions` table) with
    `revoked_at` column for server-side revocation.
  - For API use (frontend AJAX and third-party public API), the
    server additionally issues **JWT access tokens** with a 15-minute
    TTL, signed with a rotating key (EdDSA Ed25519), carrying the
    session ID, user ID, and active organization ID.
  - **Refresh**: silent via the session cookie. No refresh token on
    the wire — the cookie *is* the refresh credential. Rotation on
    every refresh.
- **Machine clients**: **service account tokens** and **personal
  access tokens** (PATs), prefixed `ksa_` / `kpat_`, stored hashed,
  scoped to organization and optional cluster, with explicit expiry.
- **Agent authentication**: mTLS using per-agent client certificates
  issued at cluster creation time by our internal CA, with a rotation
  workflow that allows hitless rotation (see ADR-0007 for CA hosting).
- **MFA**: WebAuthn (passkeys) supported for interactive sessions.
  Enforced for accounts with Admin or Owner roles in Phase 3+.
- **Session lifetime**:
  - Access token: 15 minutes.
  - Session: 30 days sliding, 90 days absolute, revocable.

## Alternatives considered

- **Build our own password + email/password auth**: rejected. The
  industry has moved past this for new B2B products, and the
  password-reset / account-takeover surface is a distraction.
- **Magic-link email auth as primary**: considered as an addition for
  users who do not want to use a social IdP. Deferred to a feature flag
  pending demand.
- **Auth0 / Clerk / WorkOS as the IdP layer**: rejected as a primary
  strategy because of per-MAU pricing and lock-in. We may use WorkOS
  specifically for SCIM and directory sync in Phase 4 if the build cost
  of SCIM is prohibitive.
- **Only JWT, no server-side session**: rejected because we cannot
  revoke JWTs without a session denylist, which is functionally the
  same as a server-side session.

## Consequences

**We accept:**

- We depend on external IdPs' availability for login. Multiple IdPs
  mitigate single-provider outages, and a user with accounts linked to
  multiple IdPs can use any of them. A complete IdP outage still
  degrades login for some users; we document this in the runbook.
- JWT signing key rotation is a real operational concern. We use a
  two-key ring (active + previous) and rotate on a 90-day cycle;
  tokens signed by the previous key remain valid until expiry.
- WebAuthn enforcement for admins requires us to support device
  lifecycle (register, revoke, recover) — non-trivial but necessary.

**We assume (bets):**

- A majority of our target users have a GitHub or Google identity they
  are willing to use for signup. Early conversion data will validate.
- WorkOS (or equivalent) becomes justifiable only when we have
  validated enterprise demand; we do not build on top of it from day
  zero.

**Positive follow-ons:**

- No password database = one fewer catastrophic breach surface.
- The session-ID-in-cookie + JWT-for-API split lets us have clean
  revocation *and* stateless RPC middleware.

## Revisit trigger

- A significant share of early signups (> 20%) abandon at the IdP
  selection step because their preferred IdP is not supported, **or**
- An enterprise deal is blocked on SCIM / directory sync that we have
  not yet implemented, triggering a WorkOS evaluation.

## References

- [OpenID Connect Core 1.0](https://openid.net/specs/openid-connect-core-1_0.html)
- [WebAuthn Level 3](https://www.w3.org/TR/webauthn-3/)
- [JWT Best Current Practices — RFC 8725](https://www.rfc-editor.org/rfc/rfc8725)
- [OWASP Session Management Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
