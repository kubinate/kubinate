# Store and retrieve Hetzner API tokens under envelope encryption

**Labels**: `area/identity`, `area/integrations`, `area/security`, `sprint-1`
**Size**: 5
**Epic**: Phase-1 thin slice

## Context

A user must give us a Hetzner API token before we can provision on their
behalf. The token is tier-1 sensitive — ADR-0007 mandates envelope
encryption with a KEK loaded from environment, and the threat model
(Flow 2) requires that the token is never logged and is materialized as
plaintext only inside the Hetzner client call scope.

## Acceptance criteria

- **Given** an authenticated user, **when** they POST their Hetzner API
  token to `/v1/integrations/hetzner`, **then** the token is encrypted
  via pgcrypto envelope encryption and a `SecretRef` is stored in the
  `hetzner_credentials` table.
- **Given** a stored credential, **when** the cluster orchestrator
  needs it, **then** it retrieves plaintext via a `SecretStore::get`
  call and the plaintext is not visible in any log at any log level.
- **Given** an attempt to `Debug`-print a `SecretString`, **then** the
  output is literally `[REDACTED]` (verified by a unit test).
- **Given** a stored credential, **when** it is deleted, **then** both
  the ciphertext and the wrapped DEK are zeroed in the DB.

## Implementation notes

- New migration `20260501NNNNNN_hetzner_credentials.sql` with RLS.
- `integrations::hetzner::Client::new(store: Arc<dyn SecretStore>, handle: SecretRef)`.
- Rate limit submit endpoint: 10/min per user.

## Definition of Ready
- [x] AC in Given/When/Then
- [x] ADR-0007 referenced
- [x] No UI mock needed (settings form text-only for now)

## Definition of Done
- [ ] Unit test for `SecretString` redaction
- [ ] Integration test that round-trips a token via pgcrypto
- [ ] Integration test that cross-tenant access is rejected by RLS
- [ ] Audit log entry on create, update, delete (see issue 09)
- [ ] No token fields on any `tracing` event at any level (grep
      assertion in CI)
