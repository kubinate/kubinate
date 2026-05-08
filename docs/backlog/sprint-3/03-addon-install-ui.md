# Frontend addon install + status panel

**Labels**: `area/frontend`, `sprint-3`
**Epic**: Sprint 2 ticket 07 carry-over
**Size**: 3

## Context

Sprint 2 ticket 07 shipped the backend for `POST /v1/clusters/:id/addons`
+ `GET /v1/clusters/:id/addons` but left the UI as a follow-up. Today
the cluster status page renders pending / ready / failed but doesn't
expose the addon catalog or the install button.

## Acceptance criteria

- **Given** a Ready cluster, **when** the user views its status page,
  **then** a new "Add-ons" panel renders the result of
  `GET /v1/clusters/:id/addons` with each row's status badge.
- **Given** the panel, **when** the user clicks "Install ingress-nginx"
  (or a future cataloged addon), **then** a confirmation modal shows
  the chart version + repo from `GET /v1/catalog/clusters` and
  `POST`s to `/v1/clusters/:id/addons` on confirm.
- **Given** an in-flight install, **when** the polling interval fires,
  **then** the row's badge updates from `installing` → `ready` /
  `failed` without a full page reload.

## Implementation notes

- Reuse `routes/app/clusters/[id]/+page.svelte`'s 2-second polling
  interval; extend the same fetch to also call `listAddons`.
- New `lib/api/addons.ts` mirrors `clusters.ts` / `team.ts` shape.
- Modal component is a single-purpose `<dialog>` element; no design
  system rebuild.

## DoD

- [ ] Vitest: empty addon list → "Install ingress-nginx" CTA visible;
      install click triggers the POST with the right body.
- [ ] Vitest: a `failed` row renders the `status_reason` (after the
      ticket-06 sanitisation map) — never raw Helm error text.
- [ ] `npm run check` + `npm run lint` clean.
