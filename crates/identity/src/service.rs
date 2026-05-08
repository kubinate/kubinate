//! Service layer — transactional use-cases that compose repositories
//! and platform primitives (e.g. [`SecretStore`]).

use std::sync::Arc;

use kubinate_platform::{audit::AuditContext, error::PlatformError, secrets::SecretStore};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::{model::HetznerCredential, repository::HetznerCredentialRepository};

/// Use-cases for managing a tenant's Hetzner API credentials.
///
/// The service owns the lifecycle coupling between the `secrets` table
/// (ciphertext) and the `hetzner_credentials` table (user-facing row):
/// creating a credential always stores a secret first, and deleting a
/// credential always tries to zero out the secret afterwards.
pub struct HetznerCredentialService {
    repo: Arc<dyn HetznerCredentialRepository>,
    store: Arc<dyn SecretStore>,
}

impl HetznerCredentialService {
    /// Wire the service with its dependencies.
    #[must_use]
    pub fn new(repo: Arc<dyn HetznerCredentialRepository>, store: Arc<dyn SecretStore>) -> Self {
        Self { repo, store }
    }

    /// Create a new credential: encrypt the token, then insert the row.
    ///
    /// If the row insert fails (e.g. duplicate alias), the freshly
    /// stored secret is best-effort cleaned up to avoid orphans.
    pub async fn create(
        &self,
        organization_id: Uuid,
        alias: String,
        token: SecretString,
        audit: &AuditContext,
    ) -> Result<HetznerCredential, PlatformError> {
        let secret_ref = self.store.put(organization_id, token).await?;

        match self
            .repo
            .insert(organization_id, &alias, secret_ref, audit)
            .await
        {
            Ok(credential) => Ok(credential),
            Err(err) => {
                if let Err(cleanup) = self.store.delete(&secret_ref).await {
                    tracing::warn!(
                        error = %cleanup,
                        secret_id = %secret_ref.id,
                        "orphaned secret after hetzner_credentials insert failure",
                    );
                }
                Err(err)
            }
        }
    }

    /// List live credentials for the organization, newest first.
    pub async fn list(
        &self,
        organization_id: Uuid,
    ) -> Result<Vec<HetznerCredential>, PlatformError> {
        self.repo.list(organization_id).await
    }

    /// Resolve a single credential. Tenant-scoped via the repository's
    /// per-call `SET LOCAL`, so cross-tenant reads return `NotFound`.
    pub async fn get(
        &self,
        organization_id: Uuid,
        id: Uuid,
    ) -> Result<HetznerCredential, PlatformError> {
        self.repo.get(organization_id, id).await
    }

    /// Soft-delete the credential row and zero out the underlying secret.
    pub async fn delete(
        &self,
        organization_id: Uuid,
        id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let credential = self.repo.get(organization_id, id).await?;
        self.repo.soft_delete(organization_id, id, audit).await?;

        // Best-effort. If this fails the credential row is already
        // soft-deleted and unreachable via the service; the orphaned
        // secret will be cleaned up by a future background job.
        if let Err(err) = self.store.delete(&credential.secret_ref).await {
            tracing::warn!(
                error = %err,
                secret_id = %credential.secret_ref.id,
                credential_id = %id,
                "failed to delete underlying secret; left orphaned",
            );
        }
        Ok(())
    }
}

// =============================================================================
// Invite + membership use-cases (Sprint 2 ticket 06)
// =============================================================================

use crate::{
    model::{Invite, MembershipRole, MembershipView},
    repository::{InviteRepository, MembershipRepository},
};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};

/// Default invite TTL — 7 days from issue.
pub const INVITE_TTL: Duration = Duration::days(7);
/// Display prefix for the raw token. Surfaced in the UI; the secret
/// half is never persisted in cleartext.
pub const INVITE_TOKEN_PREFIX: &str = "kinv_";

/// Returned by [`MembershipService::create_invite`] — carries the raw
/// token **once** so the API handler can surface it to the inviting
/// user (who hands it off out-of-band). After this struct is dropped
/// the raw token is gone.
#[derive(Debug)]
pub struct CreatedInvite {
    /// The persisted invite metadata (without the secret).
    pub invite: Invite,
    /// Raw token — share-once value the API handler returns to the
    /// inviter. The recipient submits this back via accept.
    pub token: SecretString,
}

/// Errors specific to the invite path.
#[derive(Debug, thiserror::Error)]
pub enum InviteError {
    /// Caller attempted to invite a role they don't have authority for.
    #[error("not allowed: {0}")]
    Forbidden(String),
    /// Bubbles up everything else.
    #[error(transparent)]
    Platform(#[from] PlatformError),
}

impl From<InviteError> for PlatformError {
    fn from(err: InviteError) -> Self {
        match err {
            InviteError::Forbidden(msg) => PlatformError::Forbidden(msg),
            InviteError::Platform(p) => p,
        }
    }
}

/// Use-cases for managing invites + memberships.
pub struct MembershipService {
    invites: Arc<dyn InviteRepository>,
    memberships: Arc<dyn MembershipRepository>,
}

impl MembershipService {
    /// Wire the service.
    #[must_use]
    pub fn new(
        invites: Arc<dyn InviteRepository>,
        memberships: Arc<dyn MembershipRepository>,
    ) -> Self {
        Self {
            invites,
            memberships,
        }
    }

    /// Create an invite. The actor must have `owner` or `admin` in the
    /// target organization. Returns the raw token once; subsequent
    /// reads of the invite never expose it.
    pub async fn create_invite(
        &self,
        organization_id: Uuid,
        actor_user_id: Uuid,
        email: String,
        role: MembershipRole,
        audit: &AuditContext,
    ) -> Result<CreatedInvite, InviteError> {
        require_member_management(&self.memberships, organization_id, actor_user_id).await?;

        let token = generate_token();
        let token_hash = sha256_hex(token.expose_secret().as_bytes());
        let prefix_view = token_prefix(&token);

        let invite = self
            .invites
            .insert(
                organization_id,
                email.trim(),
                role,
                &token_hash,
                &prefix_view,
                actor_user_id,
                OffsetDateTime::now_utc() + INVITE_TTL,
                audit,
            )
            .await?;

        Ok(CreatedInvite { invite, token })
    }

    /// List invites for the organization (pending + recently
    /// accepted/revoked, capped server-side).
    pub async fn list_invites(
        &self,
        organization_id: Uuid,
        actor_user_id: Uuid,
    ) -> Result<Vec<Invite>, InviteError> {
        require_member_management(&self.memberships, organization_id, actor_user_id).await?;
        Ok(self.invites.list(organization_id).await?)
    }

    /// Revoke a pending invite. Idempotent — already-revoked or
    /// already-accepted invites are silently no-op.
    pub async fn revoke_invite(
        &self,
        organization_id: Uuid,
        invite_id: Uuid,
        actor_user_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), InviteError> {
        require_member_management(&self.memberships, organization_id, actor_user_id).await?;
        Ok(self
            .invites
            .revoke(organization_id, invite_id, audit)
            .await?)
    }

    /// Accept an invite using the raw token + the post-OIDC user.
    /// The user's email is checked against the invite to prevent
    /// random users with a stolen token from joining the wrong org.
    pub async fn accept_invite(
        &self,
        token: SecretString,
        user_id: Uuid,
        user_email: &str,
        audit: &AuditContext,
    ) -> Result<Invite, InviteError> {
        let token_hash = sha256_hex(token.expose_secret().as_bytes());
        let invite = self
            .invites
            .get_by_hash(&token_hash)
            .await?
            .ok_or_else(|| InviteError::Forbidden("invite not found".into()))?;

        if !invite.is_live(OffsetDateTime::now_utc()) {
            return Err(InviteError::Forbidden(
                "invite is expired, revoked, or already accepted".into(),
            ));
        }
        if !invite.email.eq_ignore_ascii_case(user_email) {
            return Err(InviteError::Forbidden(
                "invite email does not match the logged-in user".into(),
            ));
        }

        self.invites
            .mark_accepted(invite.id, user_id, audit)
            .await?;
        self.memberships
            .insert(invite.organization_id, user_id, invite.role, audit)
            .await?;
        Ok(invite)
    }

    /// List members + last-active for the settings page.
    pub async fn list_members(
        &self,
        organization_id: Uuid,
        actor_user_id: Uuid,
    ) -> Result<Vec<MembershipView>, InviteError> {
        // Any member can read the roster (so the page renders for
        // everyone), but only owner/admin can mutate. We enforce the
        // weaker rule here.
        let role = self
            .memberships
            .role_of(organization_id, actor_user_id)
            .await?;
        if role.is_none() {
            return Err(InviteError::Forbidden(
                "not a member of this organization".into(),
            ));
        }
        Ok(self.memberships.list(organization_id).await?)
    }

    /// Update a member's role. Owner/admin only.
    pub async fn update_role(
        &self,
        organization_id: Uuid,
        target_user_id: Uuid,
        new_role: MembershipRole,
        actor_user_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), InviteError> {
        require_member_management(&self.memberships, organization_id, actor_user_id).await?;
        // Refuse to demote the last owner — leaves the org orphaned.
        if matches!(new_role, MembershipRole::Owner) {
            // promotions to owner are always fine.
        } else {
            let current = self
                .memberships
                .role_of(organization_id, target_user_id)
                .await?;
            if matches!(current, Some(MembershipRole::Owner))
                && self
                    .memberships
                    .list(organization_id)
                    .await?
                    .iter()
                    .filter(|m| matches!(m.role, MembershipRole::Owner))
                    .count()
                    <= 1
            {
                return Err(InviteError::Forbidden(
                    "cannot demote the last owner of the organization".into(),
                ));
            }
        }
        Ok(self
            .memberships
            .update_role(organization_id, target_user_id, new_role, audit)
            .await?)
    }

    /// Remove a member. Owner/admin only.
    pub async fn remove_member(
        &self,
        organization_id: Uuid,
        target_user_id: Uuid,
        actor_user_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), InviteError> {
        require_member_management(&self.memberships, organization_id, actor_user_id).await?;
        if target_user_id == actor_user_id {
            return Err(InviteError::Forbidden(
                "use the leave-org flow to remove yourself".into(),
            ));
        }
        Ok(self
            .memberships
            .soft_delete(organization_id, target_user_id, audit)
            .await?)
    }
}

async fn require_member_management(
    memberships: &Arc<dyn MembershipRepository>,
    organization_id: Uuid,
    actor_user_id: Uuid,
) -> Result<MembershipRole, InviteError> {
    let role = memberships
        .role_of(organization_id, actor_user_id)
        .await?
        .ok_or_else(|| InviteError::Forbidden("not a member of this organization".into()))?;
    if !role.can_manage_members() {
        return Err(InviteError::Forbidden(
            "only owners and admins can manage members".into(),
        ));
    }
    Ok(role)
}

/// Generate a fresh `kinv_<base64url>` token. 32 random bytes + the
/// `kinv_` prefix; ~256 bits of entropy.
fn generate_token() -> SecretString {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine};
    let mut buf = [0u8; 32];
    OsRng.fill_bytes(&mut buf);
    SecretString::from(format!("{}{}", INVITE_TOKEN_PREFIX, B64URL.encode(buf)))
}

/// SHA-256 hash for index lookup. We use a fast hash (rather than
/// argon2) because the lookup happens on every accept-by-token call
/// and the entropy of the underlying value (256 bits) makes brute
/// force pointless. The threat model here is "find the row given the
/// token", not "guess the token from the hash".
fn sha256_hex(input: &[u8]) -> Vec<u8> {
    Sha256::digest(input).to_vec()
}

/// Display-only prefix shown in the UI: `kinv_` plus the first 4
/// characters of the random portion.
fn token_prefix(token: &SecretString) -> String {
    let s = token.expose_secret();
    let take = INVITE_TOKEN_PREFIX.len() + 4;
    s.chars().take(take.min(s.len())).collect()
}
