//! RFC 7807 Problem Details response for [`PlatformError`] (ADR-0008).

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kubinate_platform::error::PlatformError;
use serde::Serialize;

/// Wrapper that makes `PlatformError` convertible into an Axum response.
pub struct ApiError(pub PlatformError);

impl<E> From<E> for ApiError
where
    E: Into<PlatformError>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

#[derive(Serialize)]
struct Problem {
    #[serde(rename = "type")]
    kind: &'static str,
    title: &'static str,
    status: u16,
    detail: String,
    /// Stable string the SPA branches on. `None` for the generic
    /// `about:blank` cases; populated when there's a structured
    /// problem code (e.g. `"mfa_required"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind, title, detail, code) = match &self.0 {
            PlatformError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                "about:blank",
                "Not Found",
                msg.clone(),
                None,
            ),
            PlatformError::Forbidden(msg) => (
                StatusCode::FORBIDDEN,
                "about:blank",
                "Forbidden",
                msg.clone(),
                None,
            ),
            PlatformError::MfaRequired(msg) => (
                // 401 (not 403) so a browser knows to surface the
                // re-authn flow. ADR-0008 leaves the choice to the
                // application; the SPA branches on `code`.
                StatusCode::UNAUTHORIZED,
                "https://kubinate.dev/problems/mfa-required",
                "MFA Required",
                msg.clone(),
                Some("mfa_required"),
            ),
            PlatformError::Invalid(msg) => (
                StatusCode::BAD_REQUEST,
                "about:blank",
                "Invalid Input",
                msg.clone(),
                None,
            ),
            PlatformError::Conflict(msg) => (
                StatusCode::CONFLICT,
                "about:blank",
                "Conflict",
                msg.clone(),
                None,
            ),
            PlatformError::Database(err) => {
                tracing::error!(error = %err, "database error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "about:blank",
                    "Internal Server Error",
                    "an unexpected error occurred".to_string(),
                    None,
                )
            }
            PlatformError::Secret(err) => {
                tracing::error!(error = %err, "secret-store error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "about:blank",
                    "Internal Server Error",
                    "secret-store failure".to_string(),
                    None,
                )
            }
            PlatformError::Internal(err) => {
                tracing::error!(error = ?err, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "about:blank",
                    "Internal Server Error",
                    "an unexpected error occurred".to_string(),
                    None,
                )
            }
        };

        let body = Problem {
            kind,
            title,
            status: status.as_u16(),
            detail,
            code,
        };
        (status, Json(body)).into_response()
    }
}
