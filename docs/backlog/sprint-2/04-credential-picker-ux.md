# Replace the credential UUID text input with a picker

**Labels**: `area/frontend`, `sprint-2`
**Epic**: Sprint 1 carry-over
**Size**: 2

## Context

Sprint 1 ticket 02's create-cluster form takes the Hetzner credential
id as a free-text UUID. That's a bug-bait UX — users will paste
strings from the wrong tenant or mistype. Sprint 1 already exposes
`GET /v1/integrations/hetzner` (list) which returns a non-sensitive
view of every live credential.

## Acceptance criteria

- **Given** a logged-in user with at least one stored Hetzner
  credential, **when** they open `/app/clusters/new`, **then** the
  form renders a `<select>` populated from `GET
  /v1/integrations/hetzner` with `alias` as the visible label and
  `id` as the option value.
- **Given** the user has no credentials, **when** they open the form,
  **then** the picker is replaced by a "Add a Hetzner credential
  first" link to `/app/settings/integrations`.
- **Given** the picker, **when** the user selects an alias, **then**
  the existing Zod `credential_id` field is populated automatically;
  no text edit affordance.

## Implementation notes

- Reuse `loadClusterCatalog`-style fetch helper for list endpoints.
- The empty-state link target (`/app/settings/integrations`) doesn't
  need to exist yet; keep it as a stub that will be filled by the
  membership / integrations pages later in Sprint 2.

## DoD

- [ ] `npm run check` green.
- [ ] Vitest test on the form: empty list → "no credentials" affordance
      visible; non-empty list → selecting an option populates the
      hidden form field.
