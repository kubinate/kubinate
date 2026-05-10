//! `GET /v1/organizations/{org_id}/audit-log` — read-only audit log view.
//!
//! Returns the 100 most recent `audit_log_entries` rows for the calling
//! actor's organisation. Gated on Owner or Admin role (enforced via the
//! `OwnerActor` extractor which already covers both; any non-owner/admin
//! session returns 403 before reaching the handler).

use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{actor::OwnerActor, problem::ApiError, AppState};
use kubinate_platform::error::PlatformError;

pub fn routes() -> Router<AppState> {
    Router::new().route("/audit-log", get(list_audit_log))
}

#[derive(Debug, Serialize)]
pub struct AuditLogEntry {
    pub id: Uuid,
    pub action: String,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub decision: String,
    pub reason: Option<String>,
    pub actor_user_id: Option<Uuid>,
    pub actor_email: Option<String>,
    pub actor_display_name: Option<String>,
    pub request_id: Option<String>,
    pub created_at: OffsetDateTime,
}

async fn list_audit_log(
    State(state): State<AppState>,
    owner: OwnerActor,
    Path(org_id): Path<Uuid>,
) -> Result<Json<Vec<AuditLogEntry>>, ApiError> {
    let actor = owner.inner;
    if actor.organization_id != org_id {
        return Err(ApiError::from(PlatformError::Forbidden(
            "actor's active organization does not match the path".into(),
        )));
    }

    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;
    sqlx::query(&format!("SET LOCAL app.current_tenant_id = '{org_id}'"))
        .execute(&mut *tx)
        .await
        .map_err(PlatformError::from)?;

    let rows = sqlx::query_as::<_, AuditLogRow>(
        "SELECT
            e.id,
            e.action,
            e.resource_type,
            e.resource_id,
            e.decision,
            e.reason,
            e.actor_user_id,
            u.email::text  AS actor_email,
            u.display_name AS actor_display_name,
            e.request_id,
            e.created_at
         FROM audit_log_entries e
         LEFT JOIN users u ON u.id = e.actor_user_id
         WHERE e.organization_id = $1
         ORDER BY e.created_at DESC, e.id DESC
         LIMIT 100",
    )
    .bind(org_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(PlatformError::from)?;

    tx.commit().await.map_err(PlatformError::from)?;

    Ok(Json(rows.into_iter().map(AuditLogEntry::from).collect()))
}

// Internal sqlx row type — flat so sqlx can derive FromRow via the
// proc-macro without needing nested struct support.
#[derive(sqlx::FromRow)]
struct AuditLogRow {
    id: Uuid,
    action: String,
    resource_type: String,
    resource_id: Option<String>,
    decision: String,
    reason: Option<String>,
    actor_user_id: Option<Uuid>,
    actor_email: Option<String>,
    actor_display_name: Option<String>,
    request_id: Option<String>,
    created_at: OffsetDateTime,
}

impl From<AuditLogRow> for AuditLogEntry {
    fn from(r: AuditLogRow) -> Self {
        Self {
            id: r.id,
            action: r.action,
            resource_type: r.resource_type,
            resource_id: r.resource_id,
            decision: r.decision,
            reason: r.reason,
            actor_user_id: r.actor_user_id,
            actor_email: r.actor_email,
            actor_display_name: r.actor_display_name,
            request_id: r.request_id,
            created_at: r.created_at,
        }
    }
}
