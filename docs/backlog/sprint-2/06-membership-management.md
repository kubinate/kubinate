# Multi-org membership management — invite, list, role change

**Labels**: `area/identity`, `area/api`, `area/frontend`, `sprint-2`
**Size**: 5

## Context

The `memberships` table + `membership_role` enum already exist
(initial migration). The thin slice doesn't expose any way to manage
them — every user works inside the org their first OIDC login created.
Phase 2 multi-tenant work needs at minimum:

1. An invite flow (email + role).
2. A members list per org with role + last-active info.
3. Role updates and member removal.

Authz uses the existing membership table; ADR-0010 owns the rules.

## Acceptance criteria

- **Given** an `owner` of an org, **when** they POST
  `/v1/organizations/{org_id}/invites` with `{ email, role }`,
  **then** an `invite` row is created with a single-use 7-day token,
  and a transactional email is dispatched (template-only — Sprint 2
  uses a logged stub, real email lands in Sprint 3).
- **Given** the invitee logs in via OIDC and the email matches,
  **when** they accept, **then** a `memberships` row is created with
  the requested role.
- **Given** an `owner`, **when** they GET
  `/v1/organizations/{org_id}/members`, **then** they see every live
  membership with role + display_name + last login (from `sessions`).
- **Given** an `owner`, **when** they PATCH a member's role,
  **then** the change is audited.

## Implementation notes

- New migration: `invites` table, RLS-scoped, single-use token hashed
  with argon2id (same shape as `api_keys.token_hash`).
- Reuse the audit append from ticket 09 — `memberships` and `invites`
  get triggers.
- Authz: only `owner` and `admin` can invite or change roles.
- Frontend: `/app/settings/team` page with invite form + members list.

## DoD

- [ ] Integration test: invite → accept via OIDC → membership exists
      with the right role.
- [ ] Integration test: cross-tenant invite acceptance is rejected.
- [ ] Audit entries for invite create / accept / revoke; all hash-chain
      verified clean by `audit_log_verify`.
- [ ] Frontend Vitest covers role-change and remove-member happy paths.
