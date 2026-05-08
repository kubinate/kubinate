//! High-level billing operations: kick off a Checkout Session and
//! ingest a verified webhook delivery.

use std::sync::Arc;

use kubinate_integrations::stripe::{
    CheckoutSession, CheckoutSessionParams, StripeClient, StripeError,
};
use kubinate_platform::error::PlatformError;
use uuid::Uuid;

use crate::{
    model::{BillingPlan, OrgBillingState},
    repository::BillingRepository,
};

/// Plan slug → Stripe price id. Hardcoded for Sprint 2; later sprints
/// pull from a config table.
fn price_id_for_plan(plan: BillingPlan) -> Option<&'static str> {
    match plan {
        BillingPlan::Free => None,
        BillingPlan::Starter => Some("price_starter_replace_me"),
        BillingPlan::Pro => Some("price_pro_replace_me"),
    }
}

/// Errors specific to the billing service.
#[derive(Debug, thiserror::Error)]
pub enum BillingError {
    /// User asked to upgrade to `free` (which makes no sense).
    #[error("not allowed: {0}")]
    Forbidden(String),
    /// Stripe API returned an error.
    #[error(transparent)]
    Stripe(#[from] StripeError),
    /// Bubble through everything else.
    #[error(transparent)]
    Platform(#[from] PlatformError),
}

impl From<BillingError> for PlatformError {
    fn from(err: BillingError) -> Self {
        match err {
            BillingError::Forbidden(msg) => PlatformError::Forbidden(msg),
            BillingError::Platform(p) => p,
            BillingError::Stripe(StripeError::Status { status, body }) => {
                tracing::warn!(stripe_status = status, %body, "stripe api returned error");
                PlatformError::Internal(anyhow::anyhow!("stripe api status {status}"))
            }
            BillingError::Stripe(StripeError::Transport(err)) => {
                PlatformError::Internal(anyhow::anyhow!("stripe transport: {err}"))
            }
        }
    }
}

/// Use-cases for the billing bounded context.
pub struct BillingService {
    repo: Arc<dyn BillingRepository>,
    stripe: Arc<dyn StripeClient>,
    success_url: String,
    cancel_url: String,
}

impl BillingService {
    /// Wire the service.
    #[must_use]
    pub fn new(
        repo: Arc<dyn BillingRepository>,
        stripe: Arc<dyn StripeClient>,
        success_url: String,
        cancel_url: String,
    ) -> Self {
        Self {
            repo,
            stripe,
            success_url,
            cancel_url,
        }
    }

    /// Look up the org's current billing state.
    ///
    /// # Errors
    /// Returns [`BillingError`] if the database query fails.
    pub async fn state(&self, organization_id: Uuid) -> Result<OrgBillingState, BillingError> {
        Ok(self.repo.get(organization_id).await?)
    }

    /// Start a Checkout Session for a paid plan. Persists the
    /// resulting `customer` id so future events can be attributed
    /// without another API round-trip.
    ///
    /// # Errors
    /// Returns [`BillingError`] if the plan has no Stripe price, the Stripe API
    /// call fails, or the database query fails.
    pub async fn start_checkout(
        &self,
        organization_id: Uuid,
        target_plan: BillingPlan,
        actor_email: String,
    ) -> Result<CheckoutSession, BillingError> {
        let price_id = price_id_for_plan(target_plan)
            .ok_or_else(|| BillingError::Forbidden("cannot upgrade to the free plan".into()))?;

        let state = self.repo.get(organization_id).await?;
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("organization_id".into(), organization_id.to_string());
        metadata.insert(
            "target_plan".into(),
            format!("{target_plan:?}").to_lowercase(),
        );

        let session = self
            .stripe
            .create_checkout_session(CheckoutSessionParams {
                price_id: price_id.to_string(),
                success_url: self.success_url.clone(),
                cancel_url: self.cancel_url.clone(),
                customer_id: state.stripe_customer_id.clone(),
                customer_email: state.stripe_customer_id.is_none().then_some(actor_email),
                metadata,
            })
            .await?;

        if let Some(ref customer_id) = session.customer {
            self.repo
                .set_stripe_customer(organization_id, customer_id)
                .await?;
        }
        Ok(session)
    }

    /// Persist a verified webhook delivery. Returns whether the event
    /// was newly written (false → duplicate retry).
    ///
    /// # Errors
    /// Returns [`BillingError`] if the database query fails.
    pub async fn record_event(
        &self,
        stripe_event_id: &str,
        event_type: &str,
        customer_id: Option<&str>,
        payload: &serde_json::Value,
    ) -> Result<bool, BillingError> {
        let organization_id = if let Some(c) = customer_id {
            self.repo.org_for_customer(c).await?
        } else {
            None
        };
        let written = self
            .repo
            .append_event(
                stripe_event_id,
                organization_id,
                event_type,
                payload,
                "processed",
            )
            .await?;
        Ok(written)
    }
}
