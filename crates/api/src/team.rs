//! `/v1/organizations/{org_id}/{members,invites}` and
//! `/v1/invites/accept` — multi-org membership management
//! (Sprint 2 ticket 06).
//!
//! Owner/admin gating is enforced inside [`MembershipService`]; this
//! module just translates HTTP surfaces into service calls and adds
//! the audit context.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Json, Router,
};
use kubinate_identity::{
    model::{Invite, MembershipRole, MembershipView},
    service::{InviteError, INVITE_TOKEN_PREFIX},
};
use kubinate_platform::error::PlatformError;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    actor::{Actor, SessionUser},
    audit_ctx,
    problem::ApiError,
    AppState,
};

/// Mount under `/v1/organizations/:org_id`.
pub fn org_routes() -> Router<AppState> {
    Router::new()
        .route("/members", get(list_members))
        .route(
            "/members/:user_id",
            patch(update_role).delete(remove_member),
        )
        .route("/invites", post(create_invite).get(list_invites))
        .route("/invites/:id", delete(revoke_invite))
}

/// Mount at `/v1/invites/accept` — pre-membership endpoint, only the
/// session cookie identifies the actor.
pub fn invite_accept_route() -> Router<AppState> {
    Router::new().route("/accept", post(accept_invite))
}

#[derive(Deserialize)]
struct CreateInviteRequest {
    email: String,
    role: MembershipRole,
}

#[derive(Serialize)]
struct CreateInviteResponse {
    invite: InviteView,
    /// Returned **once**; the inviter hands this to the recipient
    /// out-of-band. Subsequent invite reads never expose it.
    token: String,
}

#[derive(Serialize)]
struct InviteView {
    id: Uuid,
    email: String,
    role: MembershipRole,
    token_prefix: String,
    accepted_at: Option<OffsetDateTime>,
    revoked_at: Option<OffsetDateTime>,
    expires_at: OffsetDateTime,
    created_at: OffsetDateTime,
}

impl From<Invite> for InviteView {
    fn from(i: Invite) -> Self {
        Self {
            id: i.id,
            email: i.email,
            role: i.role,
            token_prefix: i.token_prefix,
            accepted_at: i.accepted_at,
            revoked_at: i.revoked_at,
            expires_at: i.expires_at,
            created_at: i.created_at,
        }
    }
}

async fn create_invite(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Path(org_id): Path<Uuid>,
    Json(req): Json<CreateInviteRequest>,
) -> Result<impl IntoResponse, ApiError> {
    require_self_org(&actor, org_id)?;
    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    let created = state
        .membership_service
        .create_invite(org_id, actor.user_id, req.email, req.role, &audit)
        .await
        .map_err(invite_to_api)?;

    let token = created.token.expose_secret().to_string();
    let view = InviteView::from(created.invite);
    Ok((
        StatusCode::CREATED,
        Json(CreateInviteResponse {
            invite: view,
            token,
        }),
    ))
}

async fn list_invites(
    State(state): State<AppState>,
    actor: Actor,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<InviteView>>, ApiError> {
    require_self_org(&actor, org_id)?;
    let invites = state
        .membership_service
        .list_invites(org_id, actor.user_id)
        .await
        .map_err(invite_to_api)?;
    Ok(Json(invites.into_iter().map(InviteView::from).collect()))
}

async fn revoke_invite(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Path((org_id, id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_self_org(&actor, org_id)?;
    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    state
        .membership_service
        .revoke_invite(org_id, id, actor.user_id, &audit)
        .await
        .map_err(invite_to_api)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_members(
    State(state): State<AppState>,
    actor: Actor,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<MembershipView>>, ApiError> {
    require_self_org(&actor, org_id)?;
    let rows = state
        .membership_service
        .list_members(org_id, actor.user_id)
        .await
        .map_err(invite_to_api)?;
    Ok(Json(rows))
}

#[derive(Deserialize)]
struct UpdateRoleRequest {
    role: MembershipRole,
}

async fn update_role(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateRoleRequest>,
) -> Result<StatusCode, ApiError> {
    require_self_org(&actor, org_id)?;
    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    state
        .membership_service
        .update_role(org_id, user_id, req.role, actor.user_id, &audit)
        .await
        .map_err(invite_to_api)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn remove_member(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, ApiError> {
    require_self_org(&actor, org_id)?;
    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    state
        .membership_service
        .remove_member(org_id, user_id, actor.user_id, &audit)
        .await
        .map_err(invite_to_api)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AcceptInviteRequest {
    token: String,
}

#[derive(Serialize)]
struct AcceptInviteResponse {
    organization_id: Uuid,
    role: MembershipRole,
}

async fn accept_invite(
    State(state): State<AppState>,
    user: SessionUser,
    headers: HeaderMap,
    Json(req): Json<AcceptInviteRequest>,
) -> Result<Json<AcceptInviteResponse>, ApiError> {
    if !req.token.starts_with(INVITE_TOKEN_PREFIX) {
        return Err(ApiError::from(PlatformError::Invalid(
            "invite token must start with the kinv_ prefix".into(),
        )));
    }
    let email = lookup_user_email(&state, user.user_id).await?;
    // Build an audit context manually since `from_actor_and_headers`
    // takes the org-aware `Actor`. No org_id yet — the user might be
    // joining their first org via this very request.
    let audit = kubinate_platform::audit::AuditContext {
        actor_user_id: Some(user.user_id).filter(|u| !u.is_nil()),
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        ip: None,
        user_agent: headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    };
    let accepted = state
        .membership_service
        .accept_invite(SecretString::from(req.token), user.user_id, &email, &audit)
        .await
        .map_err(invite_to_api)?;
    Ok(Json(AcceptInviteResponse {
        organization_id: accepted.organization_id,
        role: accepted.role,
    }))
}

fn require_self_org(actor: &Actor, org_id: Uuid) -> Result<(), ApiError> {
    if actor.organization_id != org_id {
        return Err(ApiError::from(PlatformError::Forbidden(
            "actor's active organization does not match the path".into(),
        )));
    }
    Ok(())
}

async fn lookup_user_email(state: &AppState, user_id: Uuid) -> Result<String, ApiError> {
    let row: Option<(String,)> = sqlx::query_as("SELECT email::text FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(PlatformError::from)?;
    row.map(|(e,)| e)
        .ok_or_else(|| ApiError::from(PlatformError::NotFound(format!("user/{user_id}"))))
}

fn invite_to_api(err: InviteError) -> ApiError {
    ApiError::from(PlatformError::from(err))
}
