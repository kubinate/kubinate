//! Persistence for billing state.

use async_trait::async_trait;
use kubinate_platform::error::PlatformError;
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::model::{BillingPlan, OrgBillingState, StripeWebhookEvent};

/// Read + write of billing-related fields on `organizations` and
/// append-only logging into `subscription_events`.
#[async_trait]
pub trait BillingRepository: Send + Sync {
    /// Look up plan + `stripe_customer_id` for an org.
    async fn get(&self, organization_id: Uuid) -> Result<OrgBillingState, PlatformError>;

    /// Resolve the org id from a Stripe customer id. Returns `None`
    /// for unknown customers — the webhook handler logs the event
    /// regardless, so an unknown customer is just an unattributed row.
    async fn org_for_customer(
        &self,
        stripe_customer_id: &str,
    ) -> Result<Option<Uuid>, PlatformError>;

    /// Atomically set `stripe_customer_id` on the org row. Idempotent
    /// for the same value; conflicts on a different value (we never
    /// overwrite a customer id without an explicit migration).
    async fn set_stripe_customer(
        &self,
        organization_id: Uuid,
        customer_id: &str,
    ) -> Result<(), PlatformError>;

    /// Update the plan and audit the change.
    async fn update_plan(
        &self,
        organization_id: Uuid,
        plan: BillingPlan,
    ) -> Result<(), PlatformError>;

    /// Append a webhook delivery to `subscription_events`. Idempotent
    /// on `stripe_event_id` (Stripe retries deliveries until 2xx) —
    /// duplicate deliveries return `Ok(false)` so the caller knows to
    /// short-circuit.
    async fn append_event(
        &self,
        stripe_event_id: &str,
        organization_id: Option<Uuid>,
        event_type: &str,
        payload: &serde_json::Value,
        decision: &str,
    ) -> Result<bool, PlatformError>;

    /// Most recent webhook events (operations / SRE only — Phase 2
    /// dashboard is built on top of this).
    async fn recent_events(&self, limit: i64) -> Result<Vec<StripeWebhookEvent>, PlatformError>;
}

/// Postgres-backed billing repo.
pub struct PgBillingRepository {
    pool: PgPool,
}

impl PgBillingRepository {
    /// Wrap a pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl BillingRepository for PgBillingRepository {
    async fn get(&self, organization_id: Uuid) -> Result<OrgBillingState, PlatformError> {
        // Bypass RLS: organizations.id-equality already provides scope.
        let row: Option<(BillingPlan, Option<String>)> =
            sqlx::query_as("SELECT plan, stripe_customer_id FROM organizations WHERE id = $1")
                .bind(organization_id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(|(plan, sc)| OrgBillingState {
            organization_id,
            plan,
            stripe_customer_id: sc,
        })
        .ok_or_else(|| PlatformError::NotFound(format!("organization/{organization_id}")))
    }

    async fn org_for_customer(
        &self,
        stripe_customer_id: &str,
    ) -> Result<Option<Uuid>, PlatformError> {
        let row: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM organizations WHERE stripe_customer_id = $1")
                .bind(stripe_customer_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map(|(id,)| id))
    }

    async fn set_stripe_customer(
        &self,
        organization_id: Uuid,
        customer_id: &str,
    ) -> Result<(), PlatformError> {
        let affected = sqlx::query(
            "UPDATE organizations
             SET stripe_customer_id = $2
             WHERE id = $1
               AND (stripe_customer_id IS NULL OR stripe_customer_id = $2)",
        )
        .bind(organization_id)
        .bind(customer_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        if affected == 0 {
            return Err(PlatformError::Conflict(
                "organization already linked to a different Stripe customer".into(),
            ));
        }
        Ok(())
    }

    async fn update_plan(
        &self,
        organization_id: Uuid,
        plan: BillingPlan,
    ) -> Result<(), PlatformError> {
        sqlx::query("UPDATE organizations SET plan = $2 WHERE id = $1")
            .bind(organization_id)
            .bind(plan)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn append_event(
        &self,
        stripe_event_id: &str,
        organization_id: Option<Uuid>,
        event_type: &str,
        payload: &serde_json::Value,
        decision: &str,
    ) -> Result<bool, PlatformError> {
        // ON CONFLICT DO NOTHING returns 0 rows affected when a duplicate
        // delivery races with a previous one — exactly what we want for
        // Stripe's at-least-once retry semantics.
        let id = Uuid::now_v7();
        let affected = sqlx::query(
            "INSERT INTO subscription_events
                (id, stripe_event_id, organization_id, event_type, payload, decision)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (stripe_event_id) DO NOTHING",
        )
        .bind(id)
        .bind(stripe_event_id)
        .bind(organization_id)
        .bind(event_type)
        .bind(payload)
        .bind(decision)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(affected == 1)
    }

    async fn recent_events(&self, limit: i64) -> Result<Vec<StripeWebhookEvent>, PlatformError> {
        let rows = sqlx::query(
            r"
            SELECT id, stripe_event_id, organization_id, event_type, decision, received_at
            FROM subscription_events
            ORDER BY received_at DESC
            LIMIT $1
            ",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| StripeWebhookEvent {
                id: row.get("id"),
                stripe_event_id: row.get("stripe_event_id"),
                organization_id: row.get("organization_id"),
                event_type: row.get("event_type"),
                decision: row.get("decision"),
                received_at: row.get("received_at"),
            })
            .collect())
    }
}
