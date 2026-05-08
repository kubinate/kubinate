//! The authenticated `Actor` — owner of authz decisions (ADR-0010).

use uuid::Uuid;

/// The principal driving a request. In Phase 1 the only variant is a
/// logged-in [`Actor::User`]; later phases add service accounts, PATs,
/// and the mTLS-authenticated agent.
#[derive(Debug, Clone, Copy)]
pub enum Actor {
    /// A logged-in human user, with the opaque session id that
    /// authenticated them (for revocation and audit correlation).
    User {
        /// Primary key of the user.
        user_id: Uuid,
        /// Session id that authenticated this actor.
        session_id: Uuid,
    },
}

impl Actor {
    /// The user id, whatever the flavour of principal. Every variant in
    /// Phase 1 carries one; this helper exists so call sites do not
    /// have to pattern-match everywhere.
    #[must_use]
    pub fn user_id(&self) -> Uuid {
        match self {
            Actor::User { user_id, .. } => *user_id,
        }
    }

    /// The session id that authenticated this actor.
    #[must_use]
    pub fn session_id(&self) -> Uuid {
        match self {
            Actor::User { session_id, .. } => *session_id,
        }
    }
}
