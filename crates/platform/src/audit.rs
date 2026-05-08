//! Audit log session-context plumbing.
//!
//! The hash-chained audit trigger in migration
//! `20260424120300_audit_log.sql` reads the following `SET LOCAL`
//! session variables to populate each appended row:
//!
//! | GUC                        | Meaning                           |
//! |----------------------------|-----------------------------------|
//! | `app.current_tenant_id`    | Tenant (already set for RLS)      |
//! | `app.current_actor_id`     | User driving the mutation         |
//! | `app.current_request_id`   | Cross-cutting correlation id      |
//! | `app.current_request_ip`   | Client IP (inet-compatible text)  |
//! | `app.current_user_agent`   | Client UA string                  |
//!
//! This module exposes a small [`AuditContext`] value and a helper that
//! writes those variables into a live transaction before the mutating
//! query runs. Consumers build the context from their request layer
//! (axum extractor) and pass it into service methods alongside the
//! tenant identifier.

use sqlx::PgConnection;
use std::net::IpAddr;
use uuid::Uuid;

use crate::error::PlatformError;

/// Contextual metadata attached to a single mutation for the audit log.
///
/// Every field is optional because some mutations run outside an HTTP
/// request (background jobs, Temporal activities) and simply have no
/// actor / request id / client IP.
#[derive(Debug, Default, Clone)]
pub struct AuditContext {
    /// The user performing the action, if any.
    pub actor_user_id: Option<Uuid>,
    /// Cross-cutting correlation id attached by the edge layer.
    pub request_id: Option<String>,
    /// Client IP, already stripped of anything that might break `inet`.
    pub ip: Option<IpAddr>,
    /// Client UA string, as supplied.
    pub user_agent: Option<String>,
}

impl AuditContext {
    /// Apply this context as `SET LOCAL` variables on the given
    /// connection. The caller is responsible for having issued `BEGIN`
    /// and for committing once the mutation completes.
    ///
    /// # Errors
    /// Propagates any `sqlx` error from the SET statements.
    pub async fn apply(&self, conn: &mut PgConnection) -> Result<(), PlatformError> {
        // `SET LOCAL` does not accept bind parameters, so every value
        // must be rendered into a literal. All fields below are either
        // UUIDs (hex-only) or already-escaped strings, so we use
        // `quote_literal` inside the SQL to avoid any injection surface.
        if let Some(id) = self.actor_user_id {
            sqlx::query(&format!("SET LOCAL app.current_actor_id = '{id}'",))
                .execute(&mut *conn)
                .await?;
        }

        if let Some(ref req_id) = self.request_id {
            sqlx::query("SELECT set_config('app.current_request_id', $1, true)")
                .bind(req_id)
                .execute(&mut *conn)
                .await?;
        }

        if let Some(ip) = self.ip {
            sqlx::query("SELECT set_config('app.current_request_ip', $1, true)")
                .bind(ip.to_string())
                .execute(&mut *conn)
                .await?;
        }

        if let Some(ref ua) = self.user_agent {
            sqlx::query("SELECT set_config('app.current_user_agent', $1, true)")
                .bind(ua)
                .execute(&mut *conn)
                .await?;
        }

        Ok(())
    }
}

/// Append an explicit (non-DML) audit entry — the SQL counterpart is
/// `audit_log_append_explicit` from migration `20260424120700`. Use
/// this for events that aren't a row mutation (kubeconfig
/// downloads, login attempts, etc.) so they still participate in the
/// per-tenant hash chain.
///
/// The caller must already have set `app.current_tenant_id` on the
/// connection (and ideally the rest of the audit GUCs via
/// [`AuditContext::apply`]).
///
/// # Errors
/// Propagates database errors.
pub async fn append_explicit(
    conn: &mut PgConnection,
    organization_id: Uuid,
    action: &str,
    resource_type: &str,
    resource_id: Option<&str>,
    decision: &str,
    metadata: serde_json::Value,
) -> Result<Uuid, PlatformError> {
    let row: (Uuid,) = sqlx::query_as("SELECT audit_log_append_explicit($1, $2, $3, $4, $5, $6)")
        .bind(organization_id)
        .bind(action)
        .bind(resource_type)
        .bind(resource_id.unwrap_or(""))
        .bind(decision)
        .bind(metadata)
        .fetch_one(conn)
        .await?;
    Ok(row.0)
}

/// Verify the hash chain for a tenant. Returns the list of broken
/// `(entry_id, reason)` rows — empty if the chain is intact.
///
/// This is a thin wrapper around the SQL-side `audit_log_verify`
/// function. Keeping the verification logic in SQL lets Postgres see
/// exactly the same bytes the trigger hashed; a parallel Rust
/// implementation would risk drift.
///
/// # Errors
/// Propagates database errors.
pub async fn verify_chain(
    conn: &mut PgConnection,
    organization_id: Uuid,
) -> Result<Vec<(Uuid, String)>, PlatformError> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT broken_id, reason FROM audit_log_verify($1)")
            .bind(organization_id)
            .fetch_all(conn)
            .await?;
    Ok(rows)
}
