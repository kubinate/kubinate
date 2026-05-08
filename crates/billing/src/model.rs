//! Domain types for the billing bounded context.
//!
//! Sprint 2 ticket 08 ships only what we need to track an org's plan
//! and durably log every Stripe webhook delivery; subscription
//! lifecycle, invoices, and usage records arrive in later sprints.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Mirror of the SQL `billing_plan` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "billing_plan", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum BillingPlan {
    /// Default for every new org.
    Free,
    /// Paid tier — the first one Sprint 2 lets users upgrade to.
    Starter,
    /// Higher-tier paid plan.
    Pro,
}

/// Snapshot of an organization's billing-relevant fields. Read-only on
/// the `organizations` row; mutations go through
/// [`crate::service::BillingService`] so audit + Stripe stay in sync.
#[derive(Debug, Clone)]
pub struct OrgBillingState {
    /// Owning org id.
    pub organization_id: Uuid,
    /// Current plan.
    pub plan: BillingPlan,
    /// Stripe customer id, if one has been provisioned.
    pub stripe_customer_id: Option<String>,
}

/// One row in `subscription_events`.
#[derive(Debug, Clone)]
pub struct StripeWebhookEvent {
    /// Our row id.
    pub id: Uuid,
    /// `evt_…` from Stripe.
    pub stripe_event_id: String,
    /// Tenant attribution; NULL until linked.
    pub organization_id: Option<Uuid>,
    /// Stripe event type, e.g. `checkout.session.completed`.
    pub event_type: String,
    /// `processed | ignored | failed`.
    pub decision: String,
    /// When we received the delivery.
    pub received_at: OffsetDateTime,
}
