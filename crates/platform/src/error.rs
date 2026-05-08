//! Canonical error type for the platform.
//!
//! Domain crates bubble errors up as [`PlatformError`]; the API edge
//! converts them to RFC 7807 Problem Details responses.

use thiserror::Error;

use crate::secrets::SecretError;

/// Canonical error type used across Kubinate crates.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// Requested entity was not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// The caller is not authorized for the requested action.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// The caller's session is partial: WebAuthn assertion has not
    /// been completed but the route requires `Session.mfa_satisfied
    /// = true`. The API edge maps this to a 401 with the
    /// `mfa_required` Problem Details code so the SPA can route the
    /// user to the assertion flow.
    ///
    /// Sprint 4 ticket 05; ADR-0009 §MFA names the requirement.
    #[error("mfa required: {0}")]
    MfaRequired(String),

    /// Input validation failed.
    #[error("invalid input: {0}")]
    Invalid(String),

    /// Conflict (e.g. optimistic-lock failure, duplicate).
    #[error("conflict: {0}")]
    Conflict(String),

    /// Database error.
    #[error(transparent)]
    Database(#[from] sqlx::Error),

    /// Secret-store error (envelope encryption, KEK, etc.).
    #[error(transparent)]
    Secret(#[from] SecretError),

    /// Catch-all for unexpected errors; logged with full chain.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

/// Convenience alias for fallible platform operations.
pub type PlatformResult<T> = Result<T, PlatformError>;
