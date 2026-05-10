//! `/v1/billing/{checkout,webhook}` — Sprint 2 ticket 08.
//!
//! Two routes:
//!   * `POST /checkout`  — owner-only; starts a Stripe Checkout
//!     Session and returns the hosted-checkout URL.
//!   * `POST /webhook`   — un-tenanted; Stripe POSTs here. Signature
//!     verified before any DB write.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use kubinate_billing::{model::BillingPlan, service::BillingError};
use kubinate_integrations::stripe::{verify_signature, SignatureError, STRIPE_TIMESTAMP_TOLERANCE};
use kubinate_platform::error::PlatformError;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use uuid::Uuid;

use crate::{
    actor::{Actor, OwnerActor},
    problem::ApiError,
    AppState,
};

/// Mount under `/v1/billing`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/checkout", post(start_checkout))
        .route("/webhook", post(webhook))
}

/// Mount at `/v1/organizations/:org_id/billing` — read-only state
/// endpoint the dashboard's billing settings page consumes.
pub fn org_state_route() -> Router<AppState> {
    Router::new().route("/billing", get(state))
}

#[derive(Serialize)]
struct StateResponse {
    plan: BillingPlan,
    stripe_customer_id: Option<String>,
}

async fn state(
    State(state_): State<AppState>,
    actor: Actor,
    Path(org_id): Path<Uuid>,
) -> Result<Json<StateResponse>, ApiError> {
    if actor.organization_id != org_id {
        return Err(ApiError::from(PlatformError::Forbidden(
            "actor's active organization does not match the path".into(),
        )));
    }
    // Any member of the org can see the current plan; only owners
    // can mutate. The mutation gate already lives in start_checkout.
    let s = state_
        .billing_service
        .state(org_id)
        .await
        .map_err(billing_to_api)?;
    Ok(Json(StateResponse {
        plan: s.plan,
        stripe_customer_id: s.stripe_customer_id,
    }))
}

#[derive(Deserialize)]
struct CheckoutRequest {
    plan: BillingPlan,
}

#[derive(Serialize)]
struct CheckoutResponse {
    url: String,
}

async fn start_checkout(
    State(state): State<AppState>,
    owner: OwnerActor,
    Json(req): Json<CheckoutRequest>,
) -> Result<Json<CheckoutResponse>, ApiError> {
    let actor = owner.inner;

    // We need the actor's email for the new-customer Checkout path.
    let email = sqlx::query_scalar::<_, String>("SELECT email::text FROM users WHERE id = $1")
        .bind(actor.user_id)
        .fetch_one(&state.db)
        .await
        .map_err(PlatformError::from)?;

    let session = state
        .billing_service
        .start_checkout(actor.organization_id, req.plan, email)
        .await
        .map_err(billing_to_api)?;
    Ok(Json(CheckoutResponse { url: session.url }))
}

/// Stripe webhook delivery. **Critical path** — every step before the
/// signature check is constant-time-ish and reads no DB.
async fn webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            ApiError::from(PlatformError::Forbidden(
                "missing Stripe-Signature header".into(),
            ))
        })?;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0);

    if let Err(err) = verify_signature(
        &body,
        signature,
        &state.stripe_webhook_secret,
        now,
        STRIPE_TIMESTAMP_TOLERANCE,
    ) {
        let msg = match err {
            SignatureError::Malformed => "malformed signature",
            SignatureError::Timestamp => "timestamp out of tolerance",
            SignatureError::Mismatch => "signature mismatch",
        };
        // Don't 5xx — Stripe retries 5xx but stops on 4xx, which is
        // what we want for a forged delivery.
        return Err(ApiError::from(PlatformError::Forbidden(msg.into())));
    }

    // Parse the minimum we need to attribute the event. We log the
    // *whole* payload regardless so a future reprocessor can recover
    // from anything.
    let envelope: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| ApiError::from(PlatformError::Invalid(format!("invalid json: {e}"))))?;
    let stripe_event_id = envelope
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::from(PlatformError::Invalid("missing event id".into())))?;
    let event_type = envelope
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ApiError::from(PlatformError::Invalid("missing event type".into())))?;
    let customer_id = envelope
        .get("data")
        .and_then(|d| d.get("object"))
        .and_then(|o| o.get("customer"))
        .and_then(|c| c.as_str());

    match event_type {
        "checkout.session.completed" => {
            let obj = envelope
                .get("data")
                .and_then(|d| d.get("object"))
                .ok_or_else(|| {
                    ApiError::from(PlatformError::Invalid("missing data.object".into()))
                })?;
            let meta = obj
                .get("metadata")
                .ok_or_else(|| ApiError::from(PlatformError::Invalid("missing metadata".into())))?;
            let org_id: Uuid = meta
                .get("organization_id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| {
                    ApiError::from(PlatformError::Invalid(
                        "missing or invalid metadata.organization_id".into(),
                    ))
                })?;
            let plan: BillingPlan = meta
                .get("target_plan")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_value(serde_json::Value::String(s.to_owned())).ok())
                .ok_or_else(|| {
                    ApiError::from(PlatformError::Invalid(
                        "missing or invalid metadata.target_plan".into(),
                    ))
                })?;
            state
                .billing_service
                .apply_checkout_completion(stripe_event_id, org_id, plan, &envelope)
                .await
                .map_err(billing_to_api)?;
        }
        "customer.subscription.deleted" => {
            if let Some(cid) = customer_id {
                state
                    .billing_service
                    .apply_subscription_cancelled(stripe_event_id, cid, &envelope)
                    .await
                    .map_err(billing_to_api)?;
            }
        }
        _ => {
            state
                .billing_service
                .record_event(stripe_event_id, event_type, customer_id, &envelope)
                .await
                .map_err(billing_to_api)?;
        }
    }

    Ok(StatusCode::OK)
}

fn billing_to_api(err: BillingError) -> ApiError {
    ApiError::from(PlatformError::from(err))
}
