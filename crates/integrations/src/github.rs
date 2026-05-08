//! Minimal GitHub OAuth 2.0 client used by the `/v1/auth/github/*`
//! routes (ticket 07).
//!
//! Exposes a narrow trait so the API layer can be unit-tested with a
//! fake implementation instead of hitting `api.github.com`.

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use thiserror::Error;

/// GitHub OAuth authorize endpoint (where the browser goes).
pub const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
/// GitHub OAuth token exchange endpoint (server-to-server).
pub const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
/// GitHub REST endpoint returning the authenticated user.
pub const GITHUB_USER_URL: &str = "https://api.github.com/user";

/// Errors surfaced by the client.
#[derive(Debug, Error)]
pub enum GithubError {
    /// Low-level HTTP / transport failure.
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    /// Token endpoint returned an error payload.
    #[error("token exchange failed: {0}")]
    TokenExchange(String),
    /// `/user` returned a non-success status.
    #[error("user lookup failed: status {0}")]
    UserLookup(u16),
}

/// Profile shape returned by `GET /user`, trimmed to what we store.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubProfile {
    /// Numeric GitHub user id — stable across username changes.
    pub id: u64,
    /// Login (username) — for display only.
    pub login: String,
    /// Optional full name.
    pub name: Option<String>,
    /// Optional email (GitHub may hide it; we fall back to a
    /// placeholder in the caller).
    pub email: Option<String>,
    /// Optional avatar URL.
    pub avatar_url: Option<String>,
}

/// The parts of the GitHub OAuth flow the API layer delegates to.
#[async_trait]
pub trait GithubClient: Send + Sync {
    /// Swap an authorization `code` (+ PKCE verifier) for an access
    /// token.
    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<SecretString, GithubError>;

    /// Fetch the authenticated user's profile using an access token.
    async fn fetch_profile(
        &self,
        access_token: &SecretString,
    ) -> Result<GithubProfile, GithubError>;
}

/// Real client that speaks to `api.github.com` / `github.com`.
pub struct HttpGithubClient {
    client_id: String,
    client_secret: SecretString,
    http: reqwest::Client,
}

impl HttpGithubClient {
    /// Build a client from the OAuth app's credentials.
    #[must_use]
    pub fn new(client_id: String, client_secret: SecretString) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kubinate/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest build");
        Self {
            client_id,
            client_secret,
            http,
        }
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[async_trait]
impl GithubClient for HttpGithubClient {
    async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<SecretString, GithubError> {
        let resp: TokenResponse = self
            .http
            .post(GITHUB_TOKEN_URL)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.expose_secret()),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if let Some(err) = resp.error {
            let detail = resp
                .error_description
                .unwrap_or_else(|| "no description".to_string());
            return Err(GithubError::TokenExchange(format!("{err}: {detail}")));
        }

        resp.access_token
            .map(SecretString::from)
            .ok_or_else(|| GithubError::TokenExchange("no access_token in response".into()))
    }

    async fn fetch_profile(
        &self,
        access_token: &SecretString,
    ) -> Result<GithubProfile, GithubError> {
        let resp = self
            .http
            .get(GITHUB_USER_URL)
            .bearer_auth(access_token.expose_secret())
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(GithubError::UserLookup(status.as_u16()));
        }
        Ok(resp.json::<GithubProfile>().await?)
    }
}
