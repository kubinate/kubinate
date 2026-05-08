# GitHub OIDC login + session cookie issuance

**Labels**: `area/identity`, `area/api`, `area/security`, `sprint-1`
**Size**: 5

## Context

First of the OIDC IdPs per ADR-0009. Sets up the general OIDC handling;
Google/Microsoft are straightforward follow-ups.

## Acceptance criteria

- **Given** a user clicking "Sign in with GitHub", **when** they
  complete the IdP flow, **then** Kubinate verifies the ID token,
  upserts a user + oidc identity, creates a session row, and sets
  an HttpOnly + Secure + SameSite=Strict cookie.
- **Given** a subsequent request with the cookie, **then** the API
  middleware materializes `Actor::User(user_id)` without an IdP
  round-trip.
- **Given** PKCE challenge/verifier, **then** state and nonce are
  single-use, 10-minute TTL, server-side validated.

## DoD
- [ ] Unit tests for token validation (forged signature, expired,
      wrong aud, wrong nonce, replayed state)
- [ ] Integration test against a GitHub dev app
- [ ] Session rotation on successful auth (session fixation guard)
