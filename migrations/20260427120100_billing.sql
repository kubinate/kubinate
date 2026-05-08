-- =============================================================================
-- 20260427120100_billing.sql
-- =============================================================================
-- Sprint 2 ticket 08: Stripe billing scaffolding.
--
-- Adds:
--   * `plan` + `stripe_customer_id` columns on `organizations`. Every
--     existing org defaults to `free`; `stripe_customer_id` stays NULL
--     until the first Checkout Session is created.
--   * `subscription_events` — append-only log of every Stripe webhook
--     delivery, *whether or not we acted on it*. The receiver writes
--     before any business logic so the audit trail is complete even if
--     the handler later panics.
--
-- The webhook receiver runs un-tenanted (Stripe doesn't know our orgs);
-- `subscription_events.organization_id` is filled in best-effort from
-- the `customer` field on the event payload, but is allowed to be NULL
-- for events we can't yet associate (e.g. a customer that hasn't been
-- linked yet).
-- =============================================================================

CREATE TYPE billing_plan AS ENUM ('free', 'starter', 'pro');

ALTER TABLE organizations
  ADD COLUMN plan billing_plan NOT NULL DEFAULT 'free',
  ADD COLUMN stripe_customer_id TEXT;

CREATE UNIQUE INDEX organizations_stripe_customer_idx
  ON organizations (stripe_customer_id)
  WHERE stripe_customer_id IS NOT NULL;

CREATE TABLE subscription_events (
    id                UUID PRIMARY KEY,
    -- Stripe's event id, e.g. `evt_1NoXyzAaBbCcDdEe`. Unique among
    -- delivered events; we use this to deduplicate retries (Stripe
    -- delivers the same event multiple times until we 2xx).
    stripe_event_id   TEXT NOT NULL,
    -- Resolved at write time when possible; NULL when we can't link.
    organization_id   UUID REFERENCES organizations(id) ON DELETE SET NULL,
    -- Event type, e.g. `checkout.session.completed`. Stored as text
    -- because Stripe occasionally adds new ones and we'd rather log
    -- them than reject.
    event_type        TEXT NOT NULL,
    -- Whole payload for forensics. JSONB because we frequently
    -- inspect by event_type and customer id.
    payload           JSONB NOT NULL,
    -- 'processed' | 'ignored' | 'failed'. The webhook handler sets
    -- this after dispatching; failed events stay in the table for
    -- replay.
    decision          TEXT NOT NULL DEFAULT 'processed',
    received_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX subscription_events_stripe_event_idx
  ON subscription_events (stripe_event_id);
CREATE INDEX subscription_events_org_idx
  ON subscription_events (organization_id)
  WHERE organization_id IS NOT NULL;

-- The webhook arrives un-tenanted (no `app.current_tenant_id` in the
-- session), so this table intentionally has **no** RLS — the only
-- writer is the webhook handler, which validates the signature *before*
-- inserting. Reads via the dashboard go through tenant-scoped views in
-- a future ticket.
