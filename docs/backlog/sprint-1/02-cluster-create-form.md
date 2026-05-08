# Dashboard form to submit a cluster creation request

**Labels**: `area/frontend`, `area/api`, `sprint-1`
**Size**: 3

## Context

Phase-1 thin slice: one user, one cluster, one form. Form collects
region, node count, and server type; submits to
`POST /v1/clusters` which enqueues the provisioning workflow.

## Acceptance criteria

- **Given** a logged-in user with a stored Hetzner credential,
  **when** they visit `/app/clusters/new`, **then** they see a form
  with fields: name, region (dropdown from Hetzner regions), control
  plane node count (1 only in Sprint 1), worker count (1–10), server
  type (allowlisted subset).
- **Given** a valid form submission, **when** the user clicks Create,
  **then** the API returns 202 with a `cluster_id`, and the UI
  redirects to `/app/clusters/:id` showing "Provisioning".
- **Given** an invalid submission, **when** the user submits, **then**
  field-level errors are shown inline (RFC 7807 `errors` array).

## DoD
- [ ] Zod schema on the frontend matches the OpenAPI spec
- [ ] E2E happy-path test passes against local Temporal
- [ ] Idempotency-Key header present on POST
