//! Actor extraction.
//!
//! Production path: `kubinate_session` cookie → session row → user.
//! The active organization is taken from the `X-Organization-Id`
//! header and validated against the user's memberships.
//!
//! Dev path (gated by `KUBINATE__ALLOW_HEADER_ACTOR=1`): the
//! `X-Organization-Id` header alone is accepted and stands in for a
//! logged-in user. This exists so tests and pre-OIDC smoke flows can
//! exercise tenant-scoped endpoints without a real login.

use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::AppState;

/// Name of the opaque session cookie.
pub const SESSION_COOKIE: &str = "kubinate_session";

/// Currently authenticated actor. Carries the selected organization
/// so downstream handlers (which are tenant-scoped) do not have to
/// re-derive it.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_field_names)]
pub struct Actor {
    /// The user performing the action.
    pub user_id: Uuid,
    /// Session that authenticated the request. `Uuid::nil()` in dev
    /// header mode.
    pub session_id: Uuid,
    /// Active organization — every tenant-scoped query uses this.
    pub organization_id: Uuid,
}

impl<S> FromRequestParts<S> for Actor
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let State(app_state) = State::<AppState>::from_request_parts(parts, state)
            .await
            .map_err(axum::response::IntoResponse::into_response)?;

        // 1. Try cookie-backed session.
        if let Some(session_id) = extract_session_cookie(parts) {
            if let Some(session) = kubinate_identity::session::resolve(&app_state.db, session_id)
                .await
                .map_err(|_| reject(StatusCode::INTERNAL_SERVER_ERROR, "session lookup failed"))?
            {
                let org_id =
                    resolve_organization(&app_state, &parts.headers, session.user_id).await?;
                return Ok(Actor {
                    user_id: session.user_id,
                    session_id: session.id,
                    organization_id: org_id,
                });
            }
            // Cookie present but session missing / expired → fall through
            // so the dev header path can still fire if enabled.
        }

        // 2. Dev header fallback.
        if std::env::var("KUBINATE__ALLOW_HEADER_ACTOR")
            .ok()
            .as_deref()
            == Some("1")
        {
            let org_id = parts
                .headers
                .get("x-organization-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .ok_or_else(|| {
                    reject(
                        StatusCode::UNAUTHORIZED,
                        "dev header actor: X-Organization-Id missing or not a UUID",
                    )
                })?;
            let user_id = parts
                .headers
                .get("x-actor-user-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .unwrap_or(Uuid::nil());
            return Ok(Actor {
                user_id,
                session_id: Uuid::nil(),
                organization_id: org_id,
            });
        }

        Err(reject(StatusCode::UNAUTHORIZED, "authentication required"))
    }
}

fn extract_session_cookie(parts: &Parts) -> Option<Uuid> {
    let raw = parts.headers.get("cookie").and_then(|v| v.to_str().ok())?;
    for pair in raw.split(';') {
        let pair = pair.trim();
        if let Some(rest) = pair.strip_prefix(&format!("{SESSION_COOKIE}=")) {
            return Uuid::parse_str(rest).ok();
        }
    }
    None
}

async fn resolve_organization(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    user_id: Uuid,
) -> Result<Uuid, Response> {
    if let Some(header_org) = headers
        .get("x-organization-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| Uuid::parse_str(v).ok())
    {
        let count: (i64,) = sqlx::query_as(
            "SELECT count(*) FROM memberships
             WHERE user_id = $1 AND organization_id = $2 AND deleted_at IS NULL",
        )
        .bind(user_id)
        .bind(header_org)
        .fetch_one(&state.db)
        .await
        .map_err(|_| reject(StatusCode::INTERNAL_SERVER_ERROR, "membership check failed"))?;
        if count.0 == 0 {
            return Err(reject(
                StatusCode::FORBIDDEN,
                "not a member of that organization",
            ));
        }
        return Ok(header_org);
    }

    // No explicit selection — fall back to the user's most recently
    // joined membership. Multi-org users can override via the header.
    let row: Option<(Uuid,)> = sqlx::query_as(
        "SELECT organization_id FROM memberships
         WHERE user_id = $1 AND deleted_at IS NULL
         ORDER BY created_at DESC
         LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| {
        reject(
            StatusCode::INTERNAL_SERVER_ERROR,
            "membership lookup failed",
        )
    })?;

    row.map(|(org,)| org).ok_or_else(|| {
        reject(
            StatusCode::FORBIDDEN,
            "user has no active organization membership",
        )
    })
}

fn reject(status: StatusCode, msg: &'static str) -> Response {
    (status, msg).into_response()
}

/// Lighter-weight actor used by pre-membership endpoints (e.g.
/// `POST /v1/invites/accept`). Carries the authenticated user id
/// but does not require an active organization, so a brand-new
/// OIDC user with zero memberships can still call it.
#[derive(Debug, Clone, Copy)]
pub struct SessionUser {
    /// The user performing the action.
    pub user_id: Uuid,
    /// Session id the request authenticated against. `Uuid::nil()`
    /// in dev-header mode (no real session row to update). Carrying
    /// it on the extractor lets MFA-promotion endpoints update the
    /// **specific** session that initiated the assertion rather
    /// than the user's most-recent session — a multi-device user
    /// with one full + one partial session was previously promoting
    /// the wrong one.
    pub session_id: Uuid,
}

impl<S> FromRequestParts<S> for SessionUser
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let State(app_state) = State::<AppState>::from_request_parts(parts, state)
            .await
            .map_err(axum::response::IntoResponse::into_response)?;

        if let Some(session_id) = extract_session_cookie(parts) {
            if let Some(session) = kubinate_identity::session::resolve(&app_state.db, session_id)
                .await
                .map_err(|_| reject(StatusCode::INTERNAL_SERVER_ERROR, "session lookup failed"))?
            {
                return Ok(SessionUser {
                    user_id: session.user_id,
                    session_id: session.id,
                });
            }
        }

        if std::env::var("KUBINATE__ALLOW_HEADER_ACTOR")
            .ok()
            .as_deref()
            == Some("1")
        {
            let user_id = parts
                .headers
                .get("x-actor-user-id")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| Uuid::parse_str(v).ok())
                .ok_or_else(|| {
                    reject(
                        StatusCode::UNAUTHORIZED,
                        "dev header actor: X-Actor-User-Id required for SessionUser",
                    )
                })?;
            return Ok(SessionUser {
                user_id,
                session_id: Uuid::nil(),
            });
        }

        Err(reject(StatusCode::UNAUTHORIZED, "authentication required"))
    }
}

/// Privileged actor: an [`Actor`] that *also* holds an Owner or
/// Admin role in the resolved organization, *and* whose session
/// has cleared the MFA gate (`Session.mfa_satisfied = true`).
///
/// ADR-0009 §MFA: "Enforced for accounts with Admin or Owner roles
/// in Phase 3+." All state-mutating Owner/Admin routes use this
/// extractor; read-only routes use the plain [`Actor`].
///
/// Enforced on:
/// - `POST   /v1/clusters`                              (`clusters::create`)
/// - `DELETE /v1/clusters/:id`                          (`clusters::destroy`)
/// - `POST   /v1/clusters/:id/workers`                  (`clusters::scale_workers`)
/// - `POST   /v1/clusters/:id/addons`                   (`clusters::install_addon`)
/// - `POST   /v1/billing/checkout`                      (`billing::start_checkout`)
/// - `POST   /v1/integrations/hetzner/credentials`      (`integrations::create`)
/// - `DELETE /v1/integrations/hetzner/credentials/:id`  (`integrations::remove`)
/// - `POST   /v1/organizations/:id/invites`             (`team::create_invite`)
/// - `DELETE /v1/organizations/:id/invites/:id`         (`team::revoke_invite`)
/// - `PATCH  /v1/organizations/:id/members/:id`         (`team::update_role`)
/// - `DELETE /v1/organizations/:id/members/:id`         (`team::remove_member`)
///
/// Why a single extractor rather than `Owner` + `Admin` variants:
/// the headline gate is MFA, not role granularity. Routes that need
/// Owner-only (billing) add an in-handler role check alongside this.
#[derive(Debug, Clone, Copy)]
pub struct OwnerActor {
    /// The wrapped [`Actor`]. Same shape — `OwnerActor` exists
    /// purely for the gate; downstream handlers read the user
    /// + organization id off `inner`.
    pub inner: Actor,
    /// Resolved role for this user in the active organization.
    /// Always `Owner` or `Admin` — the extractor rejects everything
    /// else. Handlers that want to differentiate between the two
    /// (e.g. an Admin-can-do-X-but-not-Y policy) read this; the
    /// destroy handler does not, hence the lint allow at the field
    /// level.
    #[allow(dead_code)]
    pub role: kubinate_identity::model::MembershipRole,
}

impl<S> FromRequestParts<S> for OwnerActor
where
    S: Send + Sync,
    AppState: axum::extract::FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let State(app_state) = State::<AppState>::from_request_parts(parts, state)
            .await
            .map_err(axum::response::IntoResponse::into_response)?;
        let actor = Actor::from_request_parts(parts, state).await?;

        // 1. MFA gate. Skip the check entirely in dev-header mode
        //    (session_id == nil) — the dev fallback already
        //    short-circuits authentication; layering an MFA gate on
        //    top would block every dev smoke test that hits an
        //    Owner/Admin route.
        if !actor.session_id.is_nil() {
            let mfa_satisfied: Option<(bool,)> = sqlx::query_as(
                "SELECT mfa_satisfied FROM sessions
                 WHERE id = $1 AND revoked_at IS NULL AND expires_at > now()",
            )
            .bind(actor.session_id)
            .fetch_optional(&app_state.db)
            .await
            .map_err(|_| {
                reject(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "session mfa-state lookup failed",
                )
            })?;
            let satisfied = mfa_satisfied.is_some_and(|(b,)| b);
            if !satisfied {
                // Map to the platform error so the Problem Details
                // mapper renders the structured `mfa_required` code.
                let err = crate::problem::ApiError::from(
                    kubinate_platform::error::PlatformError::MfaRequired(
                        "WebAuthn assertion required for this action".into(),
                    ),
                );
                return Err(err.into_response());
            }
        }

        // 2. Role gate. Single query against memberships using the
        //    same shape the billing handler uses today.
        let role: Option<(kubinate_identity::model::MembershipRole,)> = sqlx::query_as(
            "SELECT role FROM memberships
             WHERE user_id = $1 AND organization_id = $2 AND deleted_at IS NULL
             LIMIT 1",
        )
        .bind(actor.user_id)
        .bind(actor.organization_id)
        .fetch_optional(&app_state.db)
        .await
        .map_err(|_| reject(StatusCode::INTERNAL_SERVER_ERROR, "role lookup failed"))?;
        let role = role.map(|(r,)| r).ok_or_else(|| {
            reject(
                StatusCode::FORBIDDEN,
                "no membership in the active organization",
            )
        })?;
        if !matches!(
            role,
            kubinate_identity::model::MembershipRole::Owner
                | kubinate_identity::model::MembershipRole::Admin
        ) {
            return Err(reject(
                StatusCode::FORBIDDEN,
                "this action requires Owner or Admin role",
            ));
        }

        Ok(OwnerActor { inner: actor, role })
    }
}
