//! Session lifecycle: issue, lookup, revoke.
//!
//! Sessions are persisted in the `sessions` table. The session id in
//! the browser cookie is the PK. We never store the raw cookie value
//! anywhere outside the DB — the cookie IS the credential, cleared on
//! logout or on revocation.

use kubinate_platform::error::PlatformError;
use sqlx::{PgConnection, PgPool};
use std::net::IpAddr;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

/// Metadata kept on each session row. The cookie carries only the
/// session id; everything else stays server-side.
#[derive(Debug, Clone)]
pub struct Session {
    /// Primary key. This is the opaque value set in the browser cookie.
    pub id: Uuid,
    /// User that authenticated this session.
    pub user_id: Uuid,
    /// When the session was issued.
    pub created_at: OffsetDateTime,
    /// Sliding-window absolute expiry.
    pub expires_at: OffsetDateTime,
    /// Set on logout or forced revocation; once non-NULL the session
    /// no longer authenticates anything.
    pub revoked_at: Option<OffsetDateTime>,
    /// Sprint 4 ticket 05 — MFA gate. `true` means the session has
    /// satisfied the WebAuthn assertion (or the user does not require
    /// MFA at all). `false` means partial: `/v1/me` is reachable but
    /// any Owner / Admin route is denied with `mfa_required`. Defaults
    /// to `true` for users without any MFA-requiring role; written
    /// `false` at issue time only when the user holds an Owner or
    /// Admin membership.
    pub mfa_satisfied: bool,
}

/// Sliding-window session lifetime. ADR-0009 §Session lifetime.
/// 30-day sliding is the short horizon; 90-day absolute is enforced
/// by the refresh path (not implemented in this sprint).
pub const SESSION_TTL: Duration = Duration::days(30);

/// Create a fresh session for a user.
pub async fn issue(
    conn: &mut PgConnection,
    user_id: Uuid,
    user_agent: Option<&str>,
    ip: Option<IpAddr>,
) -> Result<Session, PlatformError> {
    let id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    let expires_at = now + SESSION_TTL;

    sqlx::query(
        r"
        INSERT INTO sessions (id, user_id, created_at, last_used_at, expires_at, user_agent, ip_address)
        VALUES ($1, $2, $3, $3, $4, $5, $6::inet)
        ",
    )
    .bind(id)
    .bind(user_id)
    .bind(now)
    .bind(expires_at)
    .bind(user_agent)
    .bind(ip.map(|a| a.to_string()))
    .execute(&mut *conn)
    .await?;

    Ok(Session {
        id,
        user_id,
        created_at: now,
        expires_at,
        revoked_at: None,
        // Default: the schema defaults `mfa_satisfied` to TRUE so
        // the row we just inserted carries TRUE. The MFA-required
        // path (Sprint 4 ticket 05 follow-up) issues the session
        // through a different code path that flips this to FALSE.
        mfa_satisfied: true,
    })
}

/// Issue a partial session that requires WebAuthn assertion before
/// it can authorize Owner / Admin routes. Sprint 4 ticket 05 — used
/// by the OAuth callback path when the resolved user holds an
/// MFA-requiring role.
///
/// The browser receives the session cookie immediately; the
/// `actor::Actor` extractor will let the session hit `/v1/me` and
/// the `/v1/auth/passkey/*` endpoints, but no Owner / Admin route.
/// Once the assertion ceremony succeeds, the assertion handler calls
/// [`mark_mfa_satisfied`] to flip the row.
pub async fn issue_partial_mfa(
    conn: &mut PgConnection,
    user_id: Uuid,
    user_agent: Option<&str>,
    ip: Option<IpAddr>,
) -> Result<Session, PlatformError> {
    let id = Uuid::now_v7();
    let now = OffsetDateTime::now_utc();
    let expires_at = now + SESSION_TTL;

    sqlx::query(
        r"
        INSERT INTO sessions (id, user_id, created_at, last_used_at, expires_at,
                              user_agent, ip_address, mfa_satisfied)
        VALUES ($1, $2, $3, $3, $4, $5, $6::inet, FALSE)
        ",
    )
    .bind(id)
    .bind(user_id)
    .bind(now)
    .bind(expires_at)
    .bind(user_agent)
    .bind(ip.map(|a| a.to_string()))
    .execute(&mut *conn)
    .await?;

    Ok(Session {
        id,
        user_id,
        created_at: now,
        expires_at,
        revoked_at: None,
        mfa_satisfied: false,
    })
}

/// Promote a partial-MFA session after a successful WebAuthn
/// assertion. Idempotent: re-running on an already-satisfied session
/// is a no-op (returns `Ok(())`).
pub async fn mark_mfa_satisfied(pool: &PgPool, id: Uuid) -> Result<(), PlatformError> {
    sqlx::query(
        r"
        UPDATE sessions
        SET mfa_satisfied = TRUE
        WHERE id = $1 AND revoked_at IS NULL
        ",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Revoke a session by id. Idempotent.
pub async fn revoke(pool: &PgPool, id: Uuid) -> Result<(), PlatformError> {
    sqlx::query("UPDATE sessions SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Resolve a session id from a cookie to a live session record.
///
/// Returns `Ok(None)` for unknown, expired, or revoked ids — the
/// caller treats all three as "not authenticated" without branching.
pub async fn resolve(pool: &PgPool, id: Uuid) -> Result<Option<Session>, PlatformError> {
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            OffsetDateTime,
            OffsetDateTime,
            Option<OffsetDateTime>,
            bool,
        ),
    >(
        r"
        SELECT id, user_id, created_at, expires_at, revoked_at, mfa_satisfied
        FROM sessions
        WHERE id = $1
          AND revoked_at IS NULL
          AND expires_at > now()
        ",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(id, user_id, created_at, expires_at, revoked_at, mfa_satisfied)| Session {
            id,
            user_id,
            created_at,
            expires_at,
            revoked_at,
            mfa_satisfied,
        },
    ))
}
