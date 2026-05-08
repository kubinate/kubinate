# Thread `AuditContext` from API edge through service mutations

**Labels**: `area/identity`, `area/platform`, `area/api`, `sprint-2`
**Size**: 3
**Epic**: Sprint 1 carry-over

## Context

Sprint 1 ticket 09 shipped the hash-chained audit log + a trigger that
reads `app.current_actor_id`, `app.current_request_id`,
`app.current_request_ip`, `app.current_user_agent` from session GUCs.
Today every audit row writes those columns as NULL because the API
handlers never call [`AuditContext::apply`] before the mutation
transaction commits. Hetzner-credential downloads (ticket 04's
`audit_log_append_explicit`) are the only path that does.

## Acceptance criteria

- **Given** an authenticated request that mutates `hetzner_credentials`
  or `clusters`, **when** the trigger writes the audit row, **then**
  `actor_user_id`, `request_id`, `ip_address`, and `user_agent` are
  populated from the live request.
- **Given** a background task (Temporal worker, runner) doing a
  mutation, **when** it has no HTTP request, **then** `actor_user_id`
  is NULL but `request_id` is the workflow id and `user_agent` is a
  stable label like `runner/provision`.
- **Given** the dev-header actor path
  (`KUBINATE_ALLOW_HEADER_ACTOR=1`), **when** no `X-Actor-User-Id`
  header is present, **then** `actor_user_id` stays NULL — never the
  zero UUID.

## Implementation notes

- Repository methods on `HetznerCredentialRepository` and
  `ClusterRepository` accept an `AuditContext` (or option of one) and
  call `apply(&mut tx)` before issuing DML.
- `LocalRunner` constructs an `AuditContext` with the workflow id as
  `request_id`.
- Existing per-request middleware reads the cookie session + headers
  and stashes a `RequestAuditContext` on `axum::Extension`.

## DoD

- [ ] Integration test: create credential through API → audit row has
      non-NULL `actor_user_id` and `request_id` matching the
      `x-request-id` response header.
- [ ] Integration test: runner-driven mutation → audit row has NULL
      actor but non-NULL `request_id`.
- [ ] No new `tracing` field captures named after sensitive identifiers
      (regress-checked by `scripts/check-no-token-logging.sh`).
