# Stripe customer + plan column on organizations

**Labels**: `area/billing`, `area/api`, `sprint-2`
**Size**: 5
**Epic**: Phase 2 starter

## Context

Phase 2 unlocks paid tiers. Sprint 2 lands the bare minimum so the
team can stop blocking on "billing isn't real yet" decisions:

- Every org has a `plan` (`free | starter | pro`) with `free` as the
  default, persisted on the `organizations` row.
- Every org has a Stripe `customer` id; created lazily on first plan
  upgrade attempt.
- The Stripe webhook receiver verifies signatures and writes
  `subscription_events` rows for every event, regardless of whether
  we act on them.

This sprint does **not** ship: invoices, dunning, plan-gated feature
flags, or paid-feature enforcement.

## Acceptance criteria

- **Given** a new organization, **when** it's created via the OIDC
  signup flow, **then** the row defaults to `plan = 'free'` and
  `stripe_customer_id = NULL`.
- **Given** an `owner` upgrading to `starter`, **when** they POST
  `/v1/billing/checkout`, **then** the API creates a Stripe Checkout
  Session (test mode), persists `stripe_customer_id`, and returns the
  hosted-checkout URL.
- **Given** a Stripe webhook delivery, **when** it hits
  `POST /v1/billing/webhook`, **then** the request signature is
  verified using `STRIPE_WEBHOOK_SECRET`; on failure we 401, on
  success we record the event and ack 200 within 1 s.

## Implementation notes

- Migration: add `plan` enum + `stripe_customer_id` to
  `organizations`; new `subscription_events` table (RLS-exempt — the
  webhook arrives un-tenanted, and the receiver derives the tenant
  from the Stripe customer id).
- Stripe API client: `crates/integrations/src/stripe.rs` with the
  same trait/HTTP-impl pattern as Hetzner / GitHub.
- Secrets: `KUBINATE__STRIPE_SECRET_KEY` (live test key),
  `KUBINATE__STRIPE_WEBHOOK_SECRET`.

## DoD

- [ ] Webhook signature verification unit-tested with a known-good +
      tampered payload.
- [ ] Integration test against the Stripe CLI's webhook forwarder
      (`stripe listen`) gated behind a `STRIPE_TEST_KEY` env var.
- [ ] Audit entries for `plan.changed` and `stripe.customer_created`.
- [ ] No raw card numbers / PII fields ever reach the application —
      Checkout is hosted by Stripe.
