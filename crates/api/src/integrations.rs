//! `/v1/integrations/hetzner` — store and manage tenant Hetzner tokens.
//!
//! Implements Sprint 1 ticket 01. The raw token is accepted in the
//! request body, handed to `HetznerCredentialService::create` which
//! owns the encryption + row insert, and is never logged.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, post},
    Json, Router,
};
use kubinate_identity::model::HetznerCredential;
use kubinate_platform::error::PlatformError;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{actor::Actor, audit_ctx, problem::ApiError, AppState};

/// Mount the Hetzner credential routes under `/v1/integrations/hetzner`.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(create).get(list))
        .route("/:id", delete(remove))
}

#[derive(Deserialize)]
struct CreateRequest {
    alias: String,
    token: String,
}

#[derive(Serialize)]
struct CredentialView {
    id: Uuid,
    alias: String,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl From<HetznerCredential> for CredentialView {
    fn from(c: HetznerCredential) -> Self {
        Self {
            id: c.id,
            alias: c.alias,
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

async fn create(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Json(req): Json<CreateRequest>,
) -> Result<impl IntoResponse, ApiError> {
    if req.alias.trim().is_empty() {
        return Err(ApiError::from(PlatformError::Invalid(
            "alias must not be empty".into(),
        )));
    }
    if req.token.trim().is_empty() {
        return Err(ApiError::from(PlatformError::Invalid(
            "token must not be empty".into(),
        )));
    }

    // Move the token into a SecretString immediately so it can't be
    // captured by a `Debug` / tracing formatter on `req` downstream.
    let token = SecretString::from(req.token);
    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    let credential = state
        .hetzner_service
        .create(actor.organization_id, req.alias, token, &audit)
        .await?;

    Ok((StatusCode::CREATED, Json(CredentialView::from(credential))))
}

async fn list(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<Json<Vec<CredentialView>>, ApiError> {
    let rows = state.hetzner_service.list(actor.organization_id).await?;
    Ok(Json(rows.into_iter().map(CredentialView::from).collect()))
}

async fn remove(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    state
        .hetzner_service
        .delete(actor.organization_id, id, &audit)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
