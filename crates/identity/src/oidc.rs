//! OIDC / OAuth 2.0 sign-in plumbing (ADR-0009).
//!
//! Handles the server-side parts that do not belong in the HTTP layer:
//!   * generating and persisting `state` + `nonce` + PKCE `code_verifier`
//!     before the browser leaves for the `IdP`,
//!   * consuming them atomically on callback (single-use, 10-min TTL),
//!   * upserting the `users` and `user_oidc_identities` rows from the
//!     `IdP`'s userinfo response.
//!
//! GitHub's OAuth 2.0 flow does not issue an `id_token`, so there is no
//! signature / aud / nonce check against a JWT here. The `state` +
//! server-side PKCE verifier are the integrity controls.

use kubinate_platform::error::PlatformError;
use sqlx::{PgConnection, PgPool};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

/// Short TTL for an authorization-request state row. Any callback
/// arriving after this window is rejected as stale.
pub const AUTH_STATE_TTL: Duration = Duration::minutes(10);

/// Persisted state associated with a single in-flight OIDC / OAuth
/// authorization request.
#[derive(Debug, Clone)]
pub struct AuthState {
    /// The opaque anti-CSRF value echoed by the `IdP`. Also the PK.
    pub state: String,
    /// Nonce bound to this request (for future OIDC providers; unused
    /// with GitHub's OAuth 2.0 flow).
    pub nonce: String,
    /// PKCE verifier; the `code_challenge` the `IdP` sees is its SHA-256.
    pub code_verifier: String,
    /// Which `IdP` this request was routed to.
    pub provider: String,
    /// Where to send the browser after a successful login.
    pub redirect_to: Option<String>,
    /// When the row expires.
    pub expires_at: OffsetDateTime,
}

/// Errors specific to the authorization-request state store.
#[derive(Debug, thiserror::Error)]
pub enum AuthStateError {
    /// No row matched the supplied `state`.
    #[error("auth state not found or already consumed")]
    NotFound,
    /// The row existed but is past its TTL.
    #[error("auth state expired")]
    Expired,
    /// The row has already been consumed (replay).
    #[error("auth state already consumed")]
    Replay,
    /// Database error.
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

impl From<AuthStateError> for PlatformError {
    fn from(err: AuthStateError) -> Self {
        match err {
            AuthStateError::NotFound | AuthStateError::Expired | AuthStateError::Replay => {
                PlatformError::Forbidden(err.to_string())
            }
            AuthStateError::Database(err) => PlatformError::Database(err),
        }
    }
}

/// Persist a fresh authorization-request state. Caller is responsible
/// for generating `state`, `nonce`, `code_verifier` (random, URL-safe).
///
/// # Errors
/// Returns [`AuthStateError`] if the database query fails.
pub async fn store(
    pool: &PgPool,
    state: &str,
    nonce: &str,
    code_verifier: &str,
    provider: &str,
    redirect_to: Option<&str>,
) -> Result<(), AuthStateError> {
    let now = OffsetDateTime::now_utc();
    let expires_at = now + AUTH_STATE_TTL;
    sqlx::query(
        r"
        INSERT INTO auth_states
            (state, nonce, code_verifier, provider, redirect_to, created_at, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ",
    )
    .bind(state)
    .bind(nonce)
    .bind(code_verifier)
    .bind(provider)
    .bind(redirect_to)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Atomically consume a state row. Succeeds exactly once per `state`;
/// subsequent calls with the same value return [`AuthStateError::Replay`],
/// and expired rows return [`AuthStateError::Expired`].
///
/// # Errors
/// Returns [`AuthStateError`] if the state is not found, expired, already
/// consumed, or the database query fails.
pub async fn consume(pool: &PgPool, state: &str) -> Result<AuthState, AuthStateError> {
    // The UPDATE ... RETURNING idiom makes the check-and-consume a
    // single round-trip. `consumed_at IS NULL` ensures replays lose
    // the race to the first consumer.
    let row = sqlx::query_as::<
        _,
        (
            String,
            String,
            String,
            String,
            Option<String>,
            OffsetDateTime,
        ),
    >(
        r"
        UPDATE auth_states
        SET consumed_at = now()
        WHERE state = $1
          AND consumed_at IS NULL
        RETURNING state, nonce, code_verifier, provider, redirect_to, expires_at
        ",
    )
    .bind(state)
    .fetch_optional(pool)
    .await?;

    let Some((state, nonce, code_verifier, provider, redirect_to, expires_at)) = row else {
        // Either never existed or was already consumed. We cannot
        // distinguish without a second query, so surface the wider
        // failure mode (`Replay` implies prior existence).
        return Err(replay_or_missing(pool, state).await?);
    };

    if expires_at <= OffsetDateTime::now_utc() {
        return Err(AuthStateError::Expired);
    }

    Ok(AuthState {
        state,
        nonce,
        code_verifier,
        provider,
        redirect_to,
        expires_at,
    })
}

async fn replay_or_missing(pool: &PgPool, state: &str) -> Result<AuthStateError, sqlx::Error> {
    let exists: Option<(bool,)> =
        sqlx::query_as("SELECT consumed_at IS NOT NULL FROM auth_states WHERE state = $1")
            .bind(state)
            .fetch_optional(pool)
            .await?;
    Ok(match exists {
        Some((true,)) => AuthStateError::Replay,
        Some((false,)) => AuthStateError::Expired,
        None => AuthStateError::NotFound,
    })
}

/// Identity information fetched from the `IdP`'s userinfo endpoint, in
/// the shape we persist.
#[derive(Debug, Clone)]
pub struct IdpUser {
    /// Stable IdP-issued subject id. For GitHub this is the numeric
    /// `id` field of the `/user` endpoint, serialized as text.
    pub subject: String,
    /// `IdP` "issuer" discriminator. For GitHub we use the literal
    /// string `"github"` rather than a URL so link lookup does not
    /// depend on GitHub's evolving OIDC discovery status.
    pub issuer: String,
    /// Preferred email address, if the `IdP` released one.
    pub email: Option<String>,
    /// Display name shown in the UI.
    pub display_name: String,
    /// Optional avatar URL.
    pub avatar_url: Option<String>,
}

/// Upsert `users` + `user_oidc_identities` for an IdP-reported identity
/// and return the resulting `user_id`. If the `(issuer, subject)` pair
/// is already linked, the existing user is returned unchanged.
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
pub async fn upsert_identity(
    conn: &mut PgConnection,
    idp: &IdpUser,
) -> Result<Uuid, PlatformError> {
    // Look up existing link first — this is the hot path after the
    // first login.
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT user_id FROM user_oidc_identities WHERE issuer = $1 AND subject = $2",
    )
    .bind(&idp.issuer)
    .bind(&idp.subject)
    .fetch_optional(&mut *conn)
    .await?;

    if let Some((user_id,)) = existing {
        return Ok(user_id);
    }

    // No link. Create a user row and the link atomically.
    let user_id = Uuid::now_v7();
    let email = idp
        .email
        .as_deref()
        .unwrap_or(&format!(
            "{sub}+{issuer}@placeholder.local",
            sub = idp.subject,
            issuer = idp.issuer
        ))
        .to_string();

    sqlx::query(
        r"
        INSERT INTO users (id, email, display_name, avatar_url, primary_oidc_sub)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (email) DO UPDATE SET
            display_name = EXCLUDED.display_name,
            avatar_url   = EXCLUDED.avatar_url,
            updated_at   = now()
        RETURNING id
        ",
    )
    .bind(user_id)
    .bind(&email)
    .bind(&idp.display_name)
    .bind(&idp.avatar_url)
    .bind(format!("{}|{}", idp.issuer, idp.subject))
    .execute(&mut *conn)
    .await?;

    // Resolve the user id after the ON CONFLICT UPSERT — the row we
    // care about might be the pre-existing one.
    let (resolved_user_id,): (Uuid,) = sqlx::query_as("SELECT id FROM users WHERE email = $1")
        .bind(&email)
        .fetch_one(&mut *conn)
        .await?;

    sqlx::query(
        r"
        INSERT INTO user_oidc_identities (id, user_id, issuer, subject, email_at_link)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (issuer, subject) DO NOTHING
        ",
    )
    .bind(Uuid::now_v7())
    .bind(resolved_user_id)
    .bind(&idp.issuer)
    .bind(&idp.subject)
    .bind(&idp.email)
    .execute(&mut *conn)
    .await?;

    Ok(resolved_user_id)
}
