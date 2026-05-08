//! Persistence layer for identity-owned entities.
//!
//! Every method opens a tenant-scoped transaction via `SET LOCAL
//! app.current_tenant_id`, so Postgres RLS enforces isolation even
//! if a caller passes the wrong `organization_id`.

use async_trait::async_trait;
use kubinate_platform::{audit::AuditContext, error::PlatformError, secrets::SecretRef};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::model::{
    HetznerCredential, Invite, MembershipRole, MembershipView, MfaRecoveryCode, Passkey,
};
use time::OffsetDateTime;

/// CRUD operations for tenant-owned Hetzner API credentials.
///
/// Mutating methods take an [`AuditContext`] so the per-table audit
/// trigger sees `actor_user_id`, `request_id`, etc. Read methods do
/// not — SELECTs don't fire the trigger.
#[async_trait]
pub trait HetznerCredentialRepository: Send + Sync {
    /// Insert a new credential row pointing at an already-stored secret.
    async fn insert(
        &self,
        organization_id: Uuid,
        alias: &str,
        secret_ref: SecretRef,
        audit: &AuditContext,
    ) -> Result<HetznerCredential, PlatformError>;

    /// List live credentials for an organization, newest first.
    async fn list(&self, organization_id: Uuid) -> Result<Vec<HetznerCredential>, PlatformError>;

    /// Fetch a single live credential by id.
    async fn get(
        &self,
        organization_id: Uuid,
        id: Uuid,
    ) -> Result<HetznerCredential, PlatformError>;

    /// Soft-delete a credential. The underlying secret is removed by
    /// the service layer so this repository stays side-effect-free
    /// against the `secrets` table.
    async fn soft_delete(
        &self,
        organization_id: Uuid,
        id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;
}

/// Postgres-backed implementation.
pub struct PgHetznerCredentialRepository {
    pool: PgPool,
}

impl PgHetznerCredentialRepository {
    /// Wrap a pool for repository use.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl HetznerCredentialRepository for PgHetznerCredentialRepository {
    async fn insert(
        &self,
        organization_id: Uuid,
        alias: &str,
        secret_ref: SecretRef,
        audit: &AuditContext,
    ) -> Result<HetznerCredential, PlatformError> {
        if secret_ref.organization_id != organization_id {
            return Err(PlatformError::Invalid(
                "secret_ref organization does not match credential organization".into(),
            ));
        }

        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;

        let row = sqlx::query(
            r"
            INSERT INTO hetzner_credentials
                (id, organization_id, alias, secret_id)
            VALUES ($1, $2, $3, $4)
            RETURNING id, organization_id, alias, secret_id,
                      created_at, updated_at, version
            ",
        )
        .bind(id)
        .bind(organization_id)
        .bind(alias)
        .bind(secret_ref.id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_conflict)?;

        tx.commit().await?;
        Ok(row_to_credential(row))
    }

    async fn list(&self, organization_id: Uuid) -> Result<Vec<HetznerCredential>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;

        let rows = sqlx::query(
            r"
            SELECT id, organization_id, alias, secret_id,
                   created_at, updated_at, version
            FROM hetzner_credentials
            WHERE deleted_at IS NULL
            ORDER BY created_at DESC
            ",
        )
        .fetch_all(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(rows.into_iter().map(row_to_credential).collect())
    }

    async fn get(
        &self,
        organization_id: Uuid,
        id: Uuid,
    ) -> Result<HetznerCredential, PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;

        let row = sqlx::query(
            r"
            SELECT id, organization_id, alias, secret_id,
                   created_at, updated_at, version
            FROM hetzner_credentials
            WHERE id = $1 AND deleted_at IS NULL
            ",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;

        tx.commit().await?;
        row.map(row_to_credential)
            .ok_or_else(|| PlatformError::NotFound(format!("hetzner_credential/{id}")))
    }

    async fn soft_delete(
        &self,
        organization_id: Uuid,
        id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;

        let affected = sqlx::query(
            r"
            UPDATE hetzner_credentials
            SET deleted_at = now(), updated_at = now(), version = version + 1
            WHERE id = $1 AND deleted_at IS NULL
            ",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        tx.commit().await?;
        if affected == 0 {
            Err(PlatformError::NotFound(format!("hetzner_credential/{id}")))
        } else {
            Ok(())
        }
    }
}

fn set_local_tenant(organization_id: Uuid) -> String {
    format!("SET LOCAL app.current_tenant_id = '{organization_id}'")
}

fn row_to_credential(row: sqlx::postgres::PgRow) -> HetznerCredential {
    let organization_id: Uuid = row.get("organization_id");
    HetznerCredential {
        id: row.get("id"),
        organization_id,
        alias: row.get("alias"),
        secret_ref: SecretRef {
            id: row.get("secret_id"),
            organization_id,
        },
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        version: row.get("version"),
    }
}

fn map_conflict(err: sqlx::Error) -> PlatformError {
    if let sqlx::Error::Database(db_err) = &err {
        if db_err.is_unique_violation() {
            return PlatformError::Conflict(
                "a Hetzner credential with this alias already exists".into(),
            );
        }
    }
    PlatformError::Database(err)
}

// =============================================================================
// Membership management (Sprint 2 ticket 06)
// =============================================================================

/// Read + write operations against `memberships`.
#[async_trait]
pub trait MembershipRepository: Send + Sync {
    /// List every live membership in `organization_id`, joined with
    /// the user row + the most recent non-revoked session.
    async fn list(&self, organization_id: Uuid) -> Result<Vec<MembershipView>, PlatformError>;

    /// Look up the requesting user's role in `organization_id`. Used
    /// by handlers to gate "owner / admin only" actions.
    async fn role_of(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<MembershipRole>, PlatformError>;

    /// Insert a new membership row. Used by the invite-accept path.
    async fn insert(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Update a member's role. The trigger emits a `memberships.updated`
    /// audit row carrying the supplied [`AuditContext`].
    async fn update_role(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Soft-delete (set `deleted_at`).
    async fn soft_delete(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;
}

/// Postgres-backed membership repo.
pub struct PgMembershipRepository {
    pool: PgPool,
}

impl PgMembershipRepository {
    /// Wrap a pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MembershipRepository for PgMembershipRepository {
    async fn list(&self, organization_id: Uuid) -> Result<Vec<MembershipView>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;

        // Join memberships → users → most-recent non-revoked session.
        // Sessions don't have organization_id so the per-user lookup
        // is the source of truth for last activity.
        let rows: Vec<(
            Uuid,
            Uuid,
            String,
            String,
            MembershipRole,
            OffsetDateTime,
            Option<OffsetDateTime>,
        )> = sqlx::query_as(
            r"
            SELECT
                m.id,
                m.user_id,
                u.email::text,
                u.display_name,
                m.role,
                m.created_at,
                (SELECT MAX(s.last_used_at) FROM sessions s
                 WHERE s.user_id = m.user_id AND s.revoked_at IS NULL)
                    AS last_active_at
            FROM memberships m
            JOIN users u ON u.id = m.user_id
            WHERE m.organization_id = $1
              AND m.deleted_at IS NULL
            ORDER BY m.created_at ASC
            ",
        )
        .bind(organization_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, user_id, email, display_name, role, joined_at, last_active_at)| {
                    MembershipView {
                        id,
                        user_id,
                        email,
                        display_name,
                        role,
                        joined_at,
                        last_active_at,
                    }
                },
            )
            .collect())
    }

    async fn role_of(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<MembershipRole>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        let row: Option<(MembershipRole,)> = sqlx::query_as(
            "SELECT role FROM memberships
             WHERE organization_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(organization_id)
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.map(|(r,)| r))
    }

    async fn insert(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO memberships (id, organization_id, user_id, role)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (organization_id, user_id) DO UPDATE
                SET role = EXCLUDED.role, deleted_at = NULL,
                    updated_at = now(), version = memberships.version + 1",
        )
        .bind(id)
        .bind(organization_id)
        .bind(user_id)
        .bind(role)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn update_role(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        role: MembershipRole,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;
        let affected = sqlx::query(
            "UPDATE memberships SET role = $3
             WHERE organization_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(organization_id)
        .bind(user_id)
        .bind(role)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if affected == 0 {
            Err(PlatformError::NotFound(format!(
                "membership user/{user_id} in org/{organization_id}"
            )))
        } else {
            Ok(())
        }
    }

    async fn soft_delete(
        &self,
        organization_id: Uuid,
        user_id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;
        sqlx::query(
            "UPDATE memberships SET deleted_at = now()
             WHERE organization_id = $1 AND user_id = $2 AND deleted_at IS NULL",
        )
        .bind(organization_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

// =============================================================================
// Invite management (Sprint 2 ticket 06)
// =============================================================================

/// Read + write operations against `invites`.
#[async_trait]
pub trait InviteRepository: Send + Sync {
    /// Insert a fresh invite. Caller has already hashed the token via
    /// argon2id.
    #[allow(clippy::too_many_arguments)]
    async fn insert(
        &self,
        organization_id: Uuid,
        email: &str,
        role: MembershipRole,
        token_hash: &[u8],
        token_prefix: &str,
        invited_by: Uuid,
        expires_at: OffsetDateTime,
        audit: &AuditContext,
    ) -> Result<Invite, PlatformError>;

    /// List pending + recently-accepted/revoked invites for the
    /// settings UI. Caller decides how to render each.
    async fn list(&self, organization_id: Uuid) -> Result<Vec<Invite>, PlatformError>;

    /// Look up by hashed token. Tenant-bypass — accept-by-token must
    /// work pre-membership (the invitee is not yet in the org).
    async fn get_by_hash(&self, token_hash: &[u8]) -> Result<Option<Invite>, PlatformError>;

    /// Mark accepted. Caller passes the user id learned from the
    /// session (post-OIDC).
    async fn mark_accepted(
        &self,
        id: Uuid,
        accepted_by: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;

    /// Mark revoked. Idempotent.
    async fn revoke(
        &self,
        organization_id: Uuid,
        id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError>;
}

/// Postgres-backed invite repo.
pub struct PgInviteRepository {
    pool: PgPool,
}

impl PgInviteRepository {
    /// Wrap a pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl InviteRepository for PgInviteRepository {
    async fn insert(
        &self,
        organization_id: Uuid,
        email: &str,
        role: MembershipRole,
        token_hash: &[u8],
        token_prefix: &str,
        invited_by: Uuid,
        expires_at: OffsetDateTime,
        audit: &AuditContext,
    ) -> Result<Invite, PlatformError> {
        let id = Uuid::now_v7();
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;
        let row: (
            Uuid,
            Uuid,
            String,
            MembershipRole,
            String,
            Option<OffsetDateTime>,
            Option<OffsetDateTime>,
            OffsetDateTime,
            OffsetDateTime,
        ) = sqlx::query_as(
            r"
            INSERT INTO invites
                (id, organization_id, email, role, token_hash, token_prefix,
                 invited_by, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, organization_id, email::text, role, token_prefix,
                      accepted_at, revoked_at, expires_at, created_at
            ",
        )
        .bind(id)
        .bind(organization_id)
        .bind(email)
        .bind(role)
        .bind(token_hash)
        .bind(token_prefix)
        .bind(invited_by)
        .bind(expires_at)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_invite_conflict)?;
        tx.commit().await?;
        Ok(row_to_invite(row))
    }

    async fn list(&self, organization_id: Uuid) -> Result<Vec<Invite>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        let rows: Vec<(
            Uuid,
            Uuid,
            String,
            MembershipRole,
            String,
            Option<OffsetDateTime>,
            Option<OffsetDateTime>,
            OffsetDateTime,
            OffsetDateTime,
        )> = sqlx::query_as(
            r"
            SELECT id, organization_id, email::text, role, token_prefix,
                   accepted_at, revoked_at, expires_at, created_at
            FROM invites
            WHERE organization_id = $1
            ORDER BY created_at DESC
            LIMIT 200
            ",
        )
        .bind(organization_id)
        .fetch_all(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(row_to_invite).collect())
    }

    async fn get_by_hash(&self, token_hash: &[u8]) -> Result<Option<Invite>, PlatformError> {
        // Accept-by-token runs pre-membership, so we must bypass RLS.
        // Phase-0 docker uses a superuser pool which bypasses; in prod
        // this query path will run as a dedicated `invite_accept` role
        // with `BYPASSRLS` (Sprint 3 follow-up).
        let row: Option<(
            Uuid,
            Uuid,
            String,
            MembershipRole,
            String,
            Option<OffsetDateTime>,
            Option<OffsetDateTime>,
            OffsetDateTime,
            OffsetDateTime,
        )> = sqlx::query_as(
            r"
            SELECT id, organization_id, email::text, role, token_prefix,
                   accepted_at, revoked_at, expires_at, created_at
            FROM invites
            WHERE token_hash = $1
            LIMIT 1
            ",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_invite))
    }

    async fn mark_accepted(
        &self,
        id: Uuid,
        accepted_by: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        // Tenant on this invite is needed for SET LOCAL — fetch first
        // (RLS-bypass via direct pool, see `get_by_hash`).
        let org: Option<(Uuid,)> =
            sqlx::query_as("SELECT organization_id FROM invites WHERE id = $1 LIMIT 1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        let Some((organization_id,)) = org else {
            return Err(PlatformError::NotFound(format!("invite/{id}")));
        };

        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;
        let affected = sqlx::query(
            "UPDATE invites
             SET accepted_at = now(), accepted_by = $2
             WHERE id = $1 AND accepted_at IS NULL AND revoked_at IS NULL
               AND expires_at > now()",
        )
        .bind(id)
        .bind(accepted_by)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        tx.commit().await?;
        if affected == 0 {
            Err(PlatformError::Forbidden(
                "invite is expired, revoked, or already accepted".into(),
            ))
        } else {
            Ok(())
        }
    }

    async fn revoke(
        &self,
        organization_id: Uuid,
        id: Uuid,
        audit: &AuditContext,
    ) -> Result<(), PlatformError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(&set_local_tenant(organization_id))
            .execute(&mut *tx)
            .await?;
        audit.apply(&mut *tx).await?;
        sqlx::query(
            "UPDATE invites SET revoked_at = now()
             WHERE id = $1 AND revoked_at IS NULL AND accepted_at IS NULL",
        )
        .bind(id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

fn row_to_invite(
    row: (
        Uuid,
        Uuid,
        String,
        MembershipRole,
        String,
        Option<OffsetDateTime>,
        Option<OffsetDateTime>,
        OffsetDateTime,
        OffsetDateTime,
    ),
) -> Invite {
    let (
        id,
        organization_id,
        email,
        role,
        token_prefix,
        accepted_at,
        revoked_at,
        expires_at,
        created_at,
    ) = row;
    Invite {
        id,
        organization_id,
        email,
        role,
        token_prefix,
        accepted_at,
        revoked_at,
        expires_at,
        created_at,
    }
}

fn map_invite_conflict(err: sqlx::Error) -> PlatformError {
    if let sqlx::Error::Database(db_err) = &err {
        if db_err.is_unique_violation() {
            return PlatformError::Conflict("an invite for this email is already pending".into());
        }
    }
    PlatformError::Database(err)
}

// ===========================================================================
// Sprint 4 ticket 05 — WebAuthn passkey + MFA recovery-code repositories.
//
// User-scoped, not tenant-scoped — see the migration's tenancy note. No
// `SET LOCAL app.current_tenant_id` call; no audit-trigger plumbing
// (sessions / users likewise have none today, and the per-event audit
// log for passkey enrol / use / revoke is a follow-up ticket).
// ===========================================================================

/// CRUD for [`Passkey`] rows. Methods take `user_id` rather than an
/// org id because passkeys live in user scope.
#[async_trait]
pub trait PasskeyRepository: Send + Sync {
    /// Persist a freshly-registered passkey. The `credential` blob is
    /// the CBOR-encoded `webauthn-rs::Passkey`; we don't deserialise
    /// it here.
    async fn insert(
        &self,
        user_id: Uuid,
        credential_id: &str,
        credential: &[u8],
        nickname: &str,
    ) -> Result<Passkey, PlatformError>;

    /// All non-revoked passkeys for the user, oldest first (so the UI
    /// renders them in the order they were registered).
    async fn list_live(&self, user_id: Uuid) -> Result<Vec<Passkey>, PlatformError>;

    /// Look up a passkey by its WebAuthn credential id. Returns
    /// `None` if no live passkey matches; the assertion ceremony's
    /// "unknown credential" branch.
    async fn find_by_credential_id(
        &self,
        credential_id: &str,
    ) -> Result<Option<Passkey>, PlatformError>;

    /// Persist the post-assertion state of a passkey: the new
    /// sign counter, the (possibly mutated) credential blob, and
    /// the `last_used_at` timestamp. webauthn-rs's
    /// `Passkey::update_credential` may mutate the blob even on
    /// successful assertions (counter, backup-eligibility flags,
    /// attestation cache), so the blob and the counter column
    /// must move in lockstep — otherwise the next assertion
    /// deserialises a stale `Passkey` and the cloned-authenticator
    /// detection silently breaks.
    ///
    /// # Errors
    /// - [`PlatformError::Conflict`] when zero rows match: the
    ///   passkey was revoked between lookup and persist, or the
    ///   counter regressed (`new_sign_counter <= stored` for
    ///   non-zero stored values; the zero-counter case is allowed
    ///   because authenticators with no counter support report `0`
    ///   indefinitely).
    /// - [`PlatformError::Database`] propagated from sqlx.
    async fn record_use(
        &self,
        passkey_id: Uuid,
        new_sign_counter: i64,
        new_credential: &[u8],
    ) -> Result<(), PlatformError>;

    /// Soft-delete a passkey. Idempotent.
    async fn revoke(&self, user_id: Uuid, passkey_id: Uuid) -> Result<(), PlatformError>;
}

/// Postgres-backed [`PasskeyRepository`].
pub struct PgPasskeyRepository {
    pool: PgPool,
}

impl PgPasskeyRepository {
    /// Wrap a pool for repository use.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PasskeyRepository for PgPasskeyRepository {
    async fn insert(
        &self,
        user_id: Uuid,
        credential_id: &str,
        credential: &[u8],
        nickname: &str,
    ) -> Result<Passkey, PlatformError> {
        let id = Uuid::now_v7();
        let row = sqlx::query(
            r"
            INSERT INTO user_passkeys
                (id, user_id, credential_id, credential, sign_counter, nickname)
            VALUES ($1, $2, $3, $4, 0, $5)
            RETURNING id, user_id, credential_id, credential, sign_counter,
                      nickname, registered_at, last_used_at, revoked_at
            ",
        )
        .bind(id)
        .bind(user_id)
        .bind(credential_id)
        .bind(credential)
        .bind(nickname)
        .fetch_one(&self.pool)
        .await
        .map_err(|err| {
            if let sqlx::Error::Database(db_err) = &err {
                if db_err.is_unique_violation() {
                    return PlatformError::Conflict(
                        "this passkey is already registered".into(),
                    );
                }
            }
            PlatformError::Database(err)
        })?;
        Ok(row_to_passkey(&row))
    }

    async fn list_live(&self, user_id: Uuid) -> Result<Vec<Passkey>, PlatformError> {
        let rows = sqlx::query(
            r"
            SELECT id, user_id, credential_id, credential, sign_counter,
                   nickname, registered_at, last_used_at, revoked_at
            FROM user_passkeys
            WHERE user_id = $1 AND revoked_at IS NULL
            ORDER BY registered_at ASC
            ",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(row_to_passkey).collect())
    }

    async fn find_by_credential_id(
        &self,
        credential_id: &str,
    ) -> Result<Option<Passkey>, PlatformError> {
        let row = sqlx::query(
            r"
            SELECT id, user_id, credential_id, credential, sign_counter,
                   nickname, registered_at, last_used_at, revoked_at
            FROM user_passkeys
            WHERE credential_id = $1 AND revoked_at IS NULL
            ",
        )
        .bind(credential_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.as_ref().map(row_to_passkey))
    }

    async fn record_use(
        &self,
        passkey_id: Uuid,
        new_sign_counter: i64,
        new_credential: &[u8],
    ) -> Result<(), PlatformError> {
        // Three things move atomically:
        //   1. sign_counter (cloned-authenticator detection).
        //   2. credential blob (webauthn-rs may mutate internal
        //      state during finish_assertion; storing it ensures
        //      future assertions deserialise the latest).
        //   3. last_used_at (UI surface).
        //
        // WHERE clause defence-in-depth (webauthn-rs already
        // checks):
        //   - revoked_at IS NULL — the passkey wasn't revoked
        //     between lookup and update.
        //   - new_sign_counter > stored OR stored = 0 — a counter
        //     can't roll back. The zero-counter case is allowed
        //     because authenticators with no counter support
        //     legitimately report 0 forever; tightening to strict
        //     `>` for those would self-DoS valid assertions.
        let result = sqlx::query(
            r"
            UPDATE user_passkeys
            SET sign_counter = $2,
                credential = $3,
                last_used_at = now()
            WHERE id = $1
              AND revoked_at IS NULL
              AND ($2 > sign_counter OR sign_counter = 0)
            ",
        )
        .bind(passkey_id)
        .bind(new_sign_counter)
        .bind(new_credential)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() != 1 {
            // Either revoked between lookup and persist, or counter
            // regression — surface as Conflict so the assertion
            // handler refuses to mark the session MFA-satisfied.
            return Err(PlatformError::Conflict(
                "passkey was revoked or the assertion counter regressed".into(),
            ));
        }
        Ok(())
    }

    async fn revoke(&self, user_id: Uuid, passkey_id: Uuid) -> Result<(), PlatformError> {
        // 404 (NotFound) when no row matches: either the passkey
        // doesn't exist, or it belongs to a different user, or
        // it's already revoked. The three cases are
        // indistinguishable to the caller — none of them admit
        // information about other users' passkeys.
        let result = sqlx::query(
            r"
            UPDATE user_passkeys
            SET revoked_at = now()
            WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL
            ",
        )
        .bind(passkey_id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(PlatformError::NotFound(format!("passkey/{passkey_id}")));
        }
        Ok(())
    }
}

fn row_to_passkey(row: &sqlx::postgres::PgRow) -> Passkey {
    Passkey {
        id: row.get("id"),
        user_id: row.get("user_id"),
        credential_id: row.get("credential_id"),
        credential: row.get("credential"),
        sign_counter: row.get("sign_counter"),
        nickname: row.get("nickname"),
        registered_at: row.get("registered_at"),
        last_used_at: row.get("last_used_at"),
        revoked_at: row.get("revoked_at"),
    }
}

/// CRUD for [`MfaRecoveryCode`] rows. The plaintext is shown to the
/// user once at enrollment; we store SHA-256 hashes only.
#[async_trait]
pub trait RecoveryCodeRepository: Send + Sync {
    /// Replace the user's batch of recovery codes with a new one.
    /// Old codes (used or not) are invalidated atomically — either
    /// the new batch lands and the old rows are gone, or nothing
    /// changes.
    async fn rotate_batch(
        &self,
        user_id: Uuid,
        code_hashes: &[[u8; 32]],
    ) -> Result<Vec<MfaRecoveryCode>, PlatformError>;

    /// Try to consume a recovery code by hash. Returns `Ok(true)` if
    /// the code was live + consumed atomically; `Ok(false)` if no
    /// matching live code exists. Never returns `Ok(true)` more than
    /// once for the same hash.
    async fn try_consume(
        &self,
        user_id: Uuid,
        code_hash: &[u8; 32],
    ) -> Result<bool, PlatformError>;

    /// Count of remaining unused codes. The UI shows this on the
    /// security settings page so a user knows when to regenerate.
    async fn count_live(&self, user_id: Uuid) -> Result<u32, PlatformError>;
}

/// Postgres-backed [`RecoveryCodeRepository`].
pub struct PgRecoveryCodeRepository {
    pool: PgPool,
}

impl PgRecoveryCodeRepository {
    /// Wrap a pool for repository use.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RecoveryCodeRepository for PgRecoveryCodeRepository {
    async fn rotate_batch(
        &self,
        user_id: Uuid,
        code_hashes: &[[u8; 32]],
    ) -> Result<Vec<MfaRecoveryCode>, PlatformError> {
        let mut tx = self.pool.begin().await?;
        // Drop every prior row — used or not. The user's intent on
        // calling this endpoint is "any code I had before this moment
        // no longer works."
        sqlx::query("DELETE FROM mfa_recovery_codes WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?;

        let mut inserted = Vec::with_capacity(code_hashes.len());
        for hash in code_hashes {
            let id = Uuid::now_v7();
            let row = sqlx::query(
                r"
                INSERT INTO mfa_recovery_codes (id, user_id, code_hash)
                VALUES ($1, $2, $3)
                RETURNING id, user_id, code_hash, created_at, used_at
                ",
            )
            .bind(id)
            .bind(user_id)
            .bind(&hash[..])
            .fetch_one(&mut *tx)
            .await?;
            inserted.push(row_to_recovery_code(&row));
        }
        tx.commit().await?;
        Ok(inserted)
    }

    async fn try_consume(
        &self,
        user_id: Uuid,
        code_hash: &[u8; 32],
    ) -> Result<bool, PlatformError> {
        // Single-statement atomic consume: the UPDATE only fires if a
        // matching live row exists, and the RETURNING confirms whether
        // we actually flipped one. Two concurrent attempts to use the
        // same code race here, and only one wins.
        let result = sqlx::query(
            r"
            UPDATE mfa_recovery_codes
            SET used_at = now()
            WHERE user_id = $1
              AND code_hash = $2
              AND used_at IS NULL
            RETURNING id
            ",
        )
        .bind(user_id)
        .bind(&code_hash[..])
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.is_some())
    }

    async fn count_live(&self, user_id: Uuid) -> Result<u32, PlatformError> {
        let (n,): (i64,) = sqlx::query_as(
            r"
            SELECT COUNT(*)::BIGINT
            FROM mfa_recovery_codes
            WHERE user_id = $1 AND used_at IS NULL
            ",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(u32::try_from(n).unwrap_or(u32::MAX))
    }
}

fn row_to_recovery_code(row: &sqlx::postgres::PgRow) -> MfaRecoveryCode {
    MfaRecoveryCode {
        id: row.get("id"),
        user_id: row.get("user_id"),
        code_hash: row.get("code_hash"),
        created_at: row.get("created_at"),
        used_at: row.get("used_at"),
    }
}
