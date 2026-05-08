//! Domain entities owned by the identity bounded context.

use kubinate_platform::secrets::SecretRef;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

/// Membership role mirroring the SQL `membership_role` enum (see the
/// initial migration). Order matters for authz: owner > admin >
/// developer > viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "membership_role", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    /// Owns the org. Can manage billing and other owners.
    Owner,
    /// Can invite, change roles, and manage clusters.
    Admin,
    /// Can create / destroy clusters but not manage members.
    Developer,
    /// Read-only.
    Viewer,
}

impl MembershipRole {
    /// Whether a holder of this role can invite or change other
    /// members' roles in their org. Centralised so handlers don't
    /// open-code the rule.
    #[must_use]
    pub fn can_manage_members(self) -> bool {
        matches!(self, MembershipRole::Owner | MembershipRole::Admin)
    }
}

/// One member of an organization, with the user-facing fields the
/// settings page renders.
#[derive(Debug, Clone, Serialize)]
pub struct MembershipView {
    /// Membership row id.
    pub id: Uuid,
    /// User id.
    pub user_id: Uuid,
    /// User's email.
    pub email: String,
    /// User's display name.
    pub display_name: String,
    /// Their role in the organization.
    pub role: MembershipRole,
    /// `created_at` on the membership row.
    pub joined_at: OffsetDateTime,
    /// Most recent `last_used_at` on a non-revoked session, if any.
    pub last_active_at: Option<OffsetDateTime>,
}

/// A pending or completed invite to an organization. The raw token
/// is never carried on this struct — only the hashed handle.
#[derive(Debug, Clone, Serialize)]
pub struct Invite {
    /// Primary key.
    pub id: Uuid,
    /// Owning organization.
    pub organization_id: Uuid,
    /// Invited email — case-insensitive (`citext`).
    pub email: String,
    /// Role the invite will grant on accept.
    pub role: MembershipRole,
    /// Display-only prefix (`kinv_…`); never the secret half.
    pub token_prefix: String,
    /// `Some` once the invitee accepts.
    pub accepted_at: Option<OffsetDateTime>,
    /// `Some` once an owner / admin revokes.
    pub revoked_at: Option<OffsetDateTime>,
    /// Hard expiry — invites past this point are rejected on accept.
    pub expires_at: OffsetDateTime,
    /// Issued time.
    pub created_at: OffsetDateTime,
}

impl Invite {
    /// Live = not yet accepted, not revoked, not past `expires_at`.
    #[must_use]
    pub fn is_live(&self, now: OffsetDateTime) -> bool {
        self.accepted_at.is_none() && self.revoked_at.is_none() && self.expires_at > now
    }
}

/// A WebAuthn passkey registered to a user. The opaque
/// `webauthn-rs`-encoded `credential` blob round-trips with the
/// assertion ceremony; we never inspect it inside this crate.
///
/// User-scoped, not tenant-scoped — a user with memberships across
/// multiple orgs uses one set of passkeys.
#[derive(Debug, Clone, Serialize)]
pub struct Passkey {
    /// Primary key.
    pub id: Uuid,
    /// The user who registered this passkey.
    pub user_id: Uuid,
    /// WebAuthn credential id (Base64URL string per the spec).
    pub credential_id: String,
    /// CBOR-encoded `webauthn-rs::Passkey` blob. Opaque outside the
    /// assertion ceremony — never deserialise this anywhere except
    /// via the webauthn-rs API.
    #[serde(skip)]
    pub credential: Vec<u8>,
    /// Sign-counter rollback detector. webauthn-rs rejects an
    /// assertion whose authenticator counter is `<=` this value
    /// (cloned-authenticator detection).
    pub sign_counter: i64,
    /// User-supplied label ("YubiKey 5C"). Capped at 64 chars in the
    /// API layer; no DB constraint to keep schema changes trivial.
    pub nickname: String,
    /// First-registration time.
    pub registered_at: OffsetDateTime,
    /// Most recent successful assertion. `None` until the passkey
    /// has been used to authenticate.
    pub last_used_at: Option<OffsetDateTime>,
    /// Revocation timestamp. A revoked passkey cannot be used to
    /// authenticate; the row is retained for the future per-user
    /// audit-log story.
    pub revoked_at: Option<OffsetDateTime>,
}

impl Passkey {
    /// Live = not revoked.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// A one-shot recovery code for the lost-device path. The plaintext
/// is shown to the user exactly once at enrollment; the row stores
/// only the SHA-256 hash. Consumption is racy across replicas — the
/// first INSERT-of-`used_at` wins; the recovery-codes repository
/// enforces this with an `UPDATE … WHERE used_at IS NULL` round-trip.
#[derive(Debug, Clone, Serialize)]
pub struct MfaRecoveryCode {
    /// Primary key.
    pub id: Uuid,
    /// Owning user.
    pub user_id: Uuid,
    /// SHA-256(plaintext). 32 bytes.
    #[serde(skip)]
    pub code_hash: Vec<u8>,
    /// Issued time.
    pub created_at: OffsetDateTime,
    /// Consumed time. `None` for unused codes.
    pub used_at: Option<OffsetDateTime>,
}

impl MfaRecoveryCode {
    /// Live = not yet used.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.used_at.is_none()
    }
}

/// A tenant's Hetzner API credential. The raw token is never stored
/// on this struct — only the handle to the ciphertext in `secrets`.
/// Kubinate's orchestrator materialises the plaintext only inside the
/// `integrations::hetzner::Client` call scope (ADR-0007, threat-model
/// Flow 2).
#[derive(Debug, Clone, Serialize)]
pub struct HetznerCredential {
    /// Primary key.
    pub id: Uuid,
    /// Owning tenant.
    pub organization_id: Uuid,
    /// User-facing label. Unique within an organization among live rows.
    pub alias: String,
    /// Handle to the envelope-encrypted token in `secrets`.
    #[serde(skip)]
    pub secret_ref: SecretRef,
    /// Creation time.
    pub created_at: OffsetDateTime,
    /// Last mutation time.
    pub updated_at: OffsetDateTime,
    /// Optimistic-lock version.
    pub version: i64,
}
