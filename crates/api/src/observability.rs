//! `/v1/observability/*` — Sprint 3 ticket 07 scaffold.
//!
//! Two routes:
//!   * `POST /write` — JSON ingest. The actor's authenticated tenant
//!     id is the only source of truth for the sample's owning org;
//!     a body claiming a different `organization_id` is rejected
//!     with `403`.
//!   * `POST /query` — tenant-scoped range query. The query body
//!     never carries an org id. The handler reads the actor's
//!     tenant from the session and passes it straight through; an
//!     attacker cannot fish for cross-tenant data even if they
//!     guess another tenant's UUID.

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use kubinate_observability::metrics::{
    handle_query, handle_write, MetricsError, RangeQuery, Sample, WriteRequest,
};
use kubinate_platform::error::PlatformError;
use serde::Serialize;

use crate::{actor::Actor, problem::ApiError, AppState};

/// Mount the observability routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/write", post(write))
        .route("/query", post(query))
}

#[derive(Serialize)]
struct WriteResponse {
    accepted: usize,
}

async fn write(
    State(state): State<AppState>,
    actor: Actor,
    Json(body): Json<WriteRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let accepted = handle_write(&state.metrics_store, actor.organization_id, body)
        .await
        .map_err(metrics_error_to_api)?;
    Ok((StatusCode::ACCEPTED, Json(WriteResponse { accepted })))
}

#[derive(Serialize)]
struct QueryResponse {
    samples: Vec<Sample>,
}

async fn query(
    State(state): State<AppState>,
    actor: Actor,
    Json(query): Json<RangeQuery>,
) -> Result<Json<QueryResponse>, ApiError> {
    let samples = handle_query(&state.metrics_store, actor.organization_id, &query)
        .await
        .map_err(metrics_error_to_api)?;
    Ok(Json(QueryResponse { samples }))
}

fn metrics_error_to_api(err: MetricsError) -> ApiError {
    match err {
        MetricsError::TenantMismatch { .. } => {
            ApiError::from(PlatformError::Forbidden(err.to_string()))
        }
        MetricsError::Backend(inner) => ApiError::from(PlatformError::Internal(inner)),
    }
}
