//! `POST /v1/auth/register` and `POST /v1/auth/login` — email + password auth.
//!
//! Supplements the GitHub OAuth flow. Users created here have no OIDC identity;
//! their credential is stored as an argon2id hash in `users.password_hash`.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use kubinate_identity::session;
use kubinate_platform::error::PlatformError;
use serde::Deserialize;
use uuid::Uuid;

use crate::{actor::SESSION_COOKIE, problem::ApiError, AppState};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
}

#[derive(Deserialize)]
struct RegisterRequest {
    email: String,
    display_name: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let email = req.email.trim().to_lowercase();
    let display_name = req.display_name.trim().to_string();
    let password = req.password;

    if email.is_empty() || !email.contains('@') {
        return Err(ApiError::from(PlatformError::Invalid(
            "valid email required".into(),
        )));
    }
    if display_name.is_empty() || display_name.len() > 100 {
        return Err(ApiError::from(PlatformError::Invalid(
            "display name must be 1–100 characters".into(),
        )));
    }
    if password.len() < 8 {
        return Err(ApiError::from(PlatformError::Invalid(
            "password must be at least 8 characters".into(),
        )));
    }

    let hash = hash_password(&password)?;

    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;

    let existing: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM users WHERE email = $1::citext")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await
        .map_err(PlatformError::from)?;

    if existing.is_some() {
        return Err(ApiError::from(PlatformError::Conflict(
            "email already registered".into(),
        )));
    }

    let user_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash)
         VALUES ($1, $2::citext, $3, $4)",
    )
    .bind(user_id)
    .bind(&email)
    .bind(&display_name)
    .bind(&hash)
    .execute(&mut *tx)
    .await
    .map_err(PlatformError::from)?;

    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let needs_partial = session::user_requires_partial_session(&state.db, user_id).await?;
    let sess = if needs_partial {
        session::issue_partial_mfa(&mut tx, user_id, ua, None).await?
    } else {
        session::issue(&mut tx, user_id, ua, None).await?
    };

    tx.commit().await.map_err(PlatformError::from)?;

    let cookie = format!(
        "{name}={value}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={max}",
        name = SESSION_COOKIE,
        value = sess.id,
        max = session::SESSION_TTL.whole_seconds(),
    );
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(header::SET_COOKIE, cookie.parse().expect("valid cookie"));
    Ok((StatusCode::CREATED, resp_headers))
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let email = req.email.trim().to_lowercase();

    let row: Option<(Uuid, Option<String>)> = sqlx::query_as(
        "SELECT id, password_hash FROM users WHERE email = $1::citext AND deleted_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await
    .map_err(PlatformError::from)?;

    // Constant-time: always attempt verify even when no row found, to
    // prevent email enumeration via timing.
    let dummy = "$argon2id$v=19$m=19456,t=2,p=1$dummysalt$dummyhash";
    let Some((user_id, Some(stored_hash))) = row else {
        let _ = verify_password("dummy", dummy);
        return Err(ApiError::from(PlatformError::Forbidden(
            "invalid email or password".into(),
        )));
    };

    if verify_password(&req.password, &stored_hash).is_err() {
        return Err(ApiError::from(PlatformError::Forbidden(
            "invalid email or password".into(),
        )));
    }

    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());

    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;
    let needs_partial = session::user_requires_partial_session(&state.db, user_id).await?;
    let sess = if needs_partial {
        session::issue_partial_mfa(&mut tx, user_id, ua, None).await?
    } else {
        session::issue(&mut tx, user_id, ua, None).await?
    };
    tx.commit().await.map_err(PlatformError::from)?;

    let cookie = format!(
        "{name}={value}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={max}",
        name = SESSION_COOKIE,
        value = sess.id,
        max = session::SESSION_TTL.whole_seconds(),
    );
    let mut resp_headers = HeaderMap::new();
    resp_headers.insert(header::SET_COOKIE, cookie.parse().expect("valid cookie"));
    Ok((StatusCode::OK, resp_headers))
}

fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::from(PlatformError::Internal(anyhow::anyhow!("argon2: {e}"))))
}

fn verify_password(password: &str, hash: &str) -> Result<(), argon2::password_hash::Error> {
    let parsed = PasswordHash::new(hash)?;
    Argon2::default().verify_password(password.as_bytes(), &parsed)
}
