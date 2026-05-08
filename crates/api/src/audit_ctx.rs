//! Build an [`AuditContext`] from per-request inputs.
//!
//! Centralised so handlers don't open-code the same field assembly.
//! Sprint 1 deferred this; ticket 01 of Sprint 2 wires it through.

use axum::http::{header, HeaderMap};
use kubinate_platform::audit::AuditContext;
use uuid::Uuid;

use crate::actor::Actor;

/// Compose an `AuditContext` from the resolved [`Actor`] and the raw
/// request headers.
///
/// * `actor_user_id` is set only when the actor carries a real user
///   id. The dev-header path leaves `Actor::user_id == Uuid::nil()`
///   when no `X-Actor-User-Id` was provided; we strip nil here so
///   audit rows never carry the zero UUID (Sprint 2 ticket 01 AC).
/// * `request_id` comes from the `x-request-id` header injected by
///   the `SetRequestIdLayer` middleware.
/// * `user_agent` from the standard request header.
/// * `ip` is intentionally `None` until we plumb `ConnectInfo` through
///   the router; we'd rather record NULL than a wrong value.
#[must_use]
pub fn from_actor_and_headers(actor: &Actor, headers: &HeaderMap) -> AuditContext {
    AuditContext {
        actor_user_id: Some(actor.user_id).filter(|u| !u.is_nil()),
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        ip: None,
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    }
}

/// Variant for endpoints that authenticate via [`crate::actor::SessionUser`]
/// rather than [`Actor`] — partial-MFA sessions that haven't bound an
/// organization yet (`/v1/auth/passkey/assert/finish`,
/// `/v1/auth/passkey/recovery/redeem`).
#[must_use]
pub fn from_user_id_and_headers(user_id: Uuid, headers: &HeaderMap) -> AuditContext {
    AuditContext {
        actor_user_id: Some(user_id).filter(|u| !u.is_nil()),
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        ip: None,
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    }
}
