//! `/v1/auth/github/*` — GitHub OAuth 2.0 sign-in routes (ticket 07).

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
use kubinate_identity::{
    oidc::{self, IdpUser},
    session,
};
use kubinate_integrations::github::{GithubClient, GITHUB_AUTHORIZE_URL};
use kubinate_platform::error::PlatformError;
use rand::RngCore;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{actor::SESSION_COOKIE, problem::ApiError, AppState};

/// IdP literal used in `user_oidc_identities.issuer` for GitHub.
const PROVIDER_GITHUB: &str = "github";

/// Mount auth routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/github/start", get(start))
        .route("/github/callback", get(callback))
        .route("/logout", axum::routing::post(logout))
}

#[derive(Deserialize)]
struct StartParams {
    /// Where to send the user after a successful callback. Validated
    /// against the configured app base URL — we never redirect to an
    /// untrusted host.
    redirect_to: Option<String>,
}

async fn start(
    State(state): State<AppState>,
    Query(params): Query<StartParams>,
) -> Result<Response, ApiError> {
    let cfg = &state.auth;

    let state_tok = random_url_safe(32);
    let nonce = random_url_safe(32);
    let code_verifier = random_url_safe(64);
    let code_challenge = B64URL.encode(Sha256::digest(code_verifier.as_bytes()));

    let redirect_to = params
        .redirect_to
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/".to_string());

    oidc::store(
        &state.db,
        &state_tok,
        &nonce,
        &code_verifier,
        PROVIDER_GITHUB,
        Some(&redirect_to),
    )
    .await
    .map_err(PlatformError::from)?;

    let mut url = url::Url::parse(GITHUB_AUTHORIZE_URL).expect("static url");
    url.query_pairs_mut()
        .append_pair("client_id", &cfg.github_client_id)
        .append_pair("redirect_uri", &cfg.github_redirect_uri)
        .append_pair("scope", "read:user user:email")
        .append_pair("state", &state_tok)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("allow_signup", "true");

    Ok(Redirect::to(url.as_str()).into_response())
}

#[derive(Deserialize)]
struct CallbackParams {
    code: String,
    state: String,
}

async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Result<Response, ApiError> {
    let cfg = &state.auth;

    let stored = oidc::consume(&state.db, &params.state)
        .await
        .map_err(PlatformError::from)?;

    if stored.provider != PROVIDER_GITHUB {
        return Err(ApiError::from(PlatformError::Forbidden(
            "auth state provider mismatch".into(),
        )));
    }

    let access_token = state
        .github
        .exchange_code(
            &params.code,
            &stored.code_verifier,
            &cfg.github_redirect_uri,
        )
        .await
        .map_err(github_to_platform)?;

    let profile = state
        .github
        .fetch_profile(&access_token)
        .await
        .map_err(github_to_platform)?;

    // Rotate any pre-existing session carried by the cookie — defence
    // against session fixation (ticket 07 DoD).
    if let Some(old) = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| extract_cookie(raw, SESSION_COOKIE))
    {
        if let Ok(old_id) = uuid::Uuid::parse_str(&old) {
            let _ = session::revoke(&state.db, old_id).await;
        }
    }

    let idp = IdpUser {
        subject: profile.id.to_string(),
        issuer: PROVIDER_GITHUB.to_string(),
        email: profile.email,
        display_name: profile.name.unwrap_or(profile.login),
        avatar_url: profile.avatar_url,
    };

    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;
    let user_id = oidc::upsert_identity(&mut tx, &idp).await?;
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok());
    let session = session::issue(&mut tx, user_id, ua, None).await?;
    tx.commit().await.map_err(PlatformError::from)?;

    let cookie = format!(
        "{name}={value}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={max_age}",
        name = SESSION_COOKIE,
        value = session.id,
        max_age = session::SESSION_TTL.whole_seconds(),
    );

    let redirect = stored.redirect_to.unwrap_or_else(|| "/".to_string());
    let mut response = Redirect::to(&redirect).into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, cookie.parse().expect("valid cookie"));
    Ok(response)
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(raw) = headers.get("cookie").and_then(|v| v.to_str().ok()) {
        if let Some(id) = extract_cookie(raw, SESSION_COOKIE) {
            if let Ok(uuid) = uuid::Uuid::parse_str(&id) {
                let _ = session::revoke(&state.db, uuid).await;
            }
        }
    }
    let clear = format!(
        "{name}=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0",
        name = SESSION_COOKIE,
    );
    let mut response = StatusCode::NO_CONTENT.into_response();
    response
        .headers_mut()
        .insert(header::SET_COOKIE, clear.parse().expect("valid cookie"));
    response
}

fn random_url_safe(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    B64URL.encode(buf)
}

fn extract_cookie(raw: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some(rest) = pair.strip_prefix(&prefix) {
            return Some(rest.to_string());
        }
    }
    None
}

fn github_to_platform(err: kubinate_integrations::github::GithubError) -> ApiError {
    match err {
        kubinate_integrations::github::GithubError::TokenExchange(msg) => ApiError::from(
            PlatformError::Forbidden(format!("github token exchange: {msg}")),
        ),
        kubinate_integrations::github::GithubError::UserLookup(status) => ApiError::from(
            PlatformError::Internal(anyhow::anyhow!("github user lookup status {status}")),
        ),
        kubinate_integrations::github::GithubError::Transport(err) => {
            ApiError::from(PlatformError::Internal(anyhow::anyhow!(err)))
        }
    }
}

/// Configuration injected into [`AppState`] for the GitHub flow.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// OAuth app client id.
    pub github_client_id: String,
    /// Fully-qualified callback URL registered with the GitHub app.
    pub github_redirect_uri: String,
}

/// The `Arc<dyn GithubClient>` alias used by `AppState`.
pub type SharedGithubClient = Arc<dyn GithubClient>;
