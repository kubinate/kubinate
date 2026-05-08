# `/app/settings/billing` — plan + checkout

**Labels**: `area/frontend`, `area/billing`, `sprint-3`
**Epic**: Sprint 2 ticket 08 carry-over
**Size**: 3

## Context

Sprint 2 ticket 08 shipped `POST /v1/billing/checkout` +
`POST /v1/billing/webhook` but no UI. The org owner needs a place to
see their current plan and upgrade.

## Acceptance criteria

- **Given** an authenticated owner, **when** they visit
  `/app/settings/billing`, **then** the page renders the org's
  current plan (free / starter / pro) and the linked Stripe
  customer id (display-only, not editable).
- **Given** the page, **when** the owner clicks "Upgrade to Starter",
  **then** the SPA POSTs to `/v1/billing/checkout` and redirects to
  the returned hosted-checkout URL.
- **Given** a non-owner member, **when** they open the page, **then**
  they see the current plan but the upgrade buttons are disabled with
  a tooltip ("only owners can change billing").

## Implementation notes

- New API surface: `GET /v1/organizations/:id/billing` returning
  `{ plan, stripe_customer_id }`. The `BillingService::state` method
  already exposes this.
- Reuse the membership-role lookup pattern from
  `routes/app/settings/team/+page.svelte` to gate the buttons.
- Empty-state copy when `plan == 'free'` differs from when the org
  is mid-checkout (Stripe customer id present, plan still `free`
  because the webhook hasn't fired).

## DoD

- [ ] Vitest: owner sees enabled upgrade buttons; non-owner sees
      disabled.
- [ ] Vitest: clicking "Upgrade to Starter" issues the POST and
      navigates to the returned URL (mocked).
- [ ] `npm run check` + `npm run lint` clean.
