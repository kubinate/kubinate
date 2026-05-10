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
    /// satisfied the `WebAuthn` assertion (or the user does not require
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
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
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

/// Issue a partial session that requires `WebAuthn` assertion before
/// it can authorize Owner / Admin routes. Sprint 4 ticket 05 — used
/// by the OAuth callback path when the resolved user holds an
/// MFA-requiring role.
///
/// The browser receives the session cookie immediately; the
/// `actor::Actor` extractor will let the session hit `/v1/me` and
/// the `/v1/auth/passkey/*` endpoints, but no Owner / Admin route.
/// Once the assertion ceremony succeeds, the assertion handler calls
/// [`mark_mfa_satisfied`] to flip the row.
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
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

/// Decide whether the OAuth callback should issue a partial session
/// for `user_id`.
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
///
/// Reads `users.requires_mfa AND users.mfa_enrolled`. The two
/// columns are deliberately decoupled:
///
/// * `requires_mfa` is the **policy bit** — does the user hold any
///   role that mandates MFA? Maintained by the
///   `memberships_requires_mfa_sync` trigger (migration
///   `20260508174335_users_requires_mfa.sql`); a role mutation
///   updates the column atomically.
/// * `mfa_enrolled` is the **state bit** — does the user have any
///   live passkey? Maintained by `PgPasskeyRepository::insert` and
///   `revoke`.
///
/// A user with `requires_mfa = TRUE AND mfa_enrolled = FALSE`
/// (Owner/Admin who hasn't enrolled yet) does **not** issue a
/// partial session: there's no credential to satisfy the
/// challenge, and forcing the partial state would lock the user
/// out of `/app/settings/security` (the only place they can
/// enrol). Sprint 5 ticket 07's SPA `mfa_state` field surfaces
/// "`must_enrol`" so the dashboard nudges the user; this helper
/// stays the gate the OAuth callback consults.
pub async fn user_requires_partial_session(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<bool, PlatformError> {
    let row: Option<(bool,)> =
        sqlx::query_as("SELECT (requires_mfa AND mfa_enrolled) FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some_and(|(b,)| b))
}

/// SPA-facing MFA state. The four values correspond to the matrix of
/// `(users.requires_mfa, users.mfa_enrolled, sessions.mfa_satisfied)`:
///
/// | `requires_mfa` | `mfa_enrolled` | `session.mfa_satisfied` | state |
/// |---|---|---|---|
/// | F | * | * | `NotRequired` |
/// | T | F | * | `MustEnrol` |
/// | T | T | F | `MustAssert` |
/// | T | T | T | `Enrolled` |
///
/// Surfaced via `/v1/me` so the dashboard can route to
/// `/app/settings/security` (`must_enrol`) or the assertion challenge
/// (`must_assert`) without inferring state from a 401 round-trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MfaState {
    /// Owner / Admin role not held; MFA is voluntary and the SPA
    /// shouldn't push the user toward enrolment.
    NotRequired,
    /// Owner / Admin role held but no passkey registered. The SPA
    /// banners a "register a passkey" prompt; the user lands on the
    /// dashboard with a full session so they can reach
    /// `/app/settings/security`.
    MustEnrol,
    /// Owner / Admin with a passkey but the current session is
    /// partial. The SPA routes to the assertion challenge.
    MustAssert,
    /// Owner / Admin, passkey registered, current session has
    /// satisfied the assertion. The dashboard runs unrestricted.
    Enrolled,
}

/// Compute [`MfaState`] for `(user_id, session_id)`.
///
/// Returns `MustAssert` when the session id doesn't resolve to a
/// live row (caller passed a stale or revoked session): the safe
/// default that nudges the SPA to re-authenticate. `Uuid::nil()`
/// for the session id (dev-header mode) is treated as
/// `mfa_satisfied = false` for the same reason — dev-header mode
/// has no real session, so `Enrolled` is not honestly representable.
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
pub async fn mfa_state(
    pool: &PgPool,
    user_id: Uuid,
    session_id: Uuid,
) -> Result<MfaState, PlatformError> {
    let user_row: Option<(bool, bool)> =
        sqlx::query_as("SELECT requires_mfa, mfa_enrolled FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let (requires_mfa, mfa_enrolled) = user_row.unwrap_or((false, false));

    if !requires_mfa {
        return Ok(MfaState::NotRequired);
    }
    if !mfa_enrolled {
        return Ok(MfaState::MustEnrol);
    }

    if session_id.is_nil() {
        return Ok(MfaState::MustAssert);
    }
    let session_row: Option<(bool,)> = sqlx::query_as(
        "SELECT mfa_satisfied FROM sessions
         WHERE id = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    let satisfied = session_row.is_some_and(|(b,)| b);
    Ok(if satisfied {
        MfaState::Enrolled
    } else {
        MfaState::MustAssert
    })
}

/// Promote a partial-MFA session after a successful `WebAuthn`
/// assertion. Idempotent: re-running on an already-satisfied session
/// is a no-op (returns `Ok(())`).
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
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
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
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
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
/// Update `last_used_at` for a session, but only if it hasn't been updated
/// in the past 5 minutes. This keeps the member list `last_active_at` field
/// meaningful without a write on every single request.
///
/// Errors are intentionally ignored by callers — a failed bump should never
/// fail the request.
pub async fn bump_activity(pool: &PgPool, id: Uuid) -> Result<(), PlatformError> {
    sqlx::query(
        "UPDATE sessions SET last_used_at = now()
         WHERE id = $1
           AND revoked_at IS NULL
           AND last_used_at < now() - interval '5 minutes'",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Look up a non-revoked, non-expired session by its primary key.
///
/// Returns `None` when the session does not exist, has expired, or has been
/// revoked — the caller treats all three cases as "unauthenticated".
///
/// # Errors
/// Returns [`PlatformError`] if the database query fails.
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
