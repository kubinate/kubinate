//! `/v1/api-keys` — personal API key management.
//!
//! Users can create long-lived `kpat_…` tokens for programmatic access.
//! This module handles management (list / create / revoke). Bearer-token
//! authentication (accepting `kpat_…` in the `Authorization` header) is
//! wired in a later sprint.
//!
//! All three routes require an authenticated session; any role may manage
//! their own keys (no Owner requirement). Tenant scope is enforced via
//! `SET LOCAL app.current_tenant_id` inside every transaction.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{actor::Actor, problem::ApiError, AppState};
use kubinate_platform::error::PlatformError;

const TOKEN_PREFIX: &str = "kpat_";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_api_keys).post(create_api_key))
        .route("/{id}", delete(revoke_api_key))
}

#[derive(Serialize)]
struct ApiKeyView {
    id: Uuid,
    name: String,
    token_prefix: String,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

#[derive(Deserialize)]
struct CreateRequest {
    name: String,
}

#[derive(Serialize)]
struct CreateApiKeyResponse {
    view: ApiKeyView,
    /// Plaintext token — shown to the user once and not stored.
    token: String,
}

/// `GET /v1/api-keys` — list the calling user's non-revoked keys.
async fn list_api_keys(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<Json<Vec<ApiKeyView>>, ApiError> {
    let org_id = actor.organization_id;
    let user_id = actor.user_id;

    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;
    sqlx::query(&format!("SET LOCAL app.current_tenant_id = '{org_id}'"))
        .execute(&mut *tx)
        .await
        .map_err(PlatformError::from)?;

    let rows = sqlx::query_as::<_, ApiKeyRow>(
        "SELECT id, name, token_prefix, created_at, last_used_at
         FROM api_keys
         WHERE user_id = $1 AND revoked_at IS NULL
         ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(PlatformError::from)?;

    tx.commit().await.map_err(PlatformError::from)?;

    Ok(Json(rows.into_iter().map(ApiKeyView::from).collect()))
}

/// `POST /v1/api-keys` — create a new personal API key.
///
/// Returns `{ view, token }`. The plaintext token is shown once and
/// cannot be retrieved again.
///
/// # Errors
/// Returns 400 if the name is empty, 500 on DB failure.
async fn create_api_key(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<CreateRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let name = req.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiError::from(PlatformError::Invalid(
            "name must not be empty".into(),
        )));
    }
    if name.len() > 64 {
        return Err(ApiError::from(PlatformError::Invalid(
            "name must be 64 characters or fewer".into(),
        )));
    }

    let token = generate_token();
    let hash = sha256(token.expose_secret().as_bytes());
    let prefix = token_prefix(&token);

    let org_id = actor.organization_id;
    let user_id = actor.user_id;
    let id = Uuid::now_v7();

    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;
    sqlx::query(&format!("SET LOCAL app.current_tenant_id = '{org_id}'"))
        .execute(&mut *tx)
        .await
        .map_err(PlatformError::from)?;

    let row = sqlx::query_as::<_, ApiKeyRow>(
        "INSERT INTO api_keys
             (id, organization_id, user_id, kind, name, token_hash, token_prefix, scopes)
         VALUES ($1, $2, $3, 'personal', $4, $5, $6, '[]')
         RETURNING id, name, token_prefix, created_at, last_used_at",
    )
    .bind(id)
    .bind(org_id)
    .bind(user_id)
    .bind(&name)
    .bind(&hash)
    .bind(&prefix)
    .fetch_one(&mut *tx)
    .await
    .map_err(PlatformError::from)?;

    tx.commit().await.map_err(PlatformError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(CreateApiKeyResponse {
            view: ApiKeyView::from(row),
            token: token.expose_secret().to_string(),
        }),
    ))
}

/// `DELETE /v1/api-keys/:id` — revoke a personal API key.
///
/// # Errors
/// Returns 404 if the key does not belong to the calling user, 500 on
/// DB failure.
async fn revoke_api_key(
    State(state): State<AppState>,
    actor: Actor,
    Path(key_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let org_id = actor.organization_id;
    let user_id = actor.user_id;

    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;
    sqlx::query(&format!("SET LOCAL app.current_tenant_id = '{org_id}'"))
        .execute(&mut *tx)
        .await
        .map_err(PlatformError::from)?;

    let affected = sqlx::query(
        "UPDATE api_keys
         SET revoked_at = now()
         WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(key_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await
    .map_err(PlatformError::from)?
    .rows_affected();

    tx.commit().await.map_err(PlatformError::from)?;

    if affected == 0 {
        return Err(ApiError::from(PlatformError::NotFound(format!(
            "api_key/{key_id}"
        ))));
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: Uuid,
    name: String,
    token_prefix: String,
    created_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

impl From<ApiKeyRow> for ApiKeyView {
    fn from(r: ApiKeyRow) -> Self {
        Self {
            id: r.id,
            name: r.name,
            token_prefix: r.token_prefix,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        }
    }
}

fn generate_token() -> SecretString {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    SecretString::from(format!("{}{}", TOKEN_PREFIX, B64URL.encode(buf)))
}

fn sha256(input: &[u8]) -> Vec<u8> {
    Sha256::digest(input).to_vec()
}

fn token_prefix(token: &SecretString) -> String {
    let s = token.expose_secret();
    let take = TOKEN_PREFIX.len() + 4;
    s.chars().take(take.min(s.len())).collect()
}
