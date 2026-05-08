//! Sprint 4 ticket 05 — `users.mfa_enrolled` flag wiring.
//!
//! These tests pin the contract the OAuth callback's
//! partial-session decision relies on:
//!
//! 1. `PgPasskeyRepository::insert` flips `users.mfa_enrolled = TRUE`
//!    in the same transaction as the passkey row insert.
//! 2. `PgPasskeyRepository::revoke` flips it back to `FALSE` once
//!    the user's last live passkey is revoked.
//! 3. `session::user_requires_partial_session` returns `true`
//!    only when both `mfa_enrolled = TRUE` and at least one
//!    Owner/Admin membership exists — so a Member-role user with
//!    a registered passkey still gets a full session (voluntary MFA
//!    stays voluntary; the gate is enforced for Owner/Admin only).
//!
//! Without these guarantees the partial-session gate either never
//! activates (flag stuck at FALSE), or locks a user out
//! permanently after they revoke their last passkey (flag stuck
//! at TRUE), or fires for the wrong role class.

use kubinate_identity::{
    model::MembershipRole,
    repository::{PasskeyRepository, PgPasskeyRepository},
    session,
};
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_org(pool: &PgPool, slug: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO organizations (id, slug, display_name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("seed org");
    id
}

async fn seed_user(pool: &PgPool, email: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(email)
        .bind(email.split('@').next().unwrap_or("user"))
        .execute(pool)
        .await
        .expect("seed user");
    id
}

async fn give_role(pool: &PgPool, org: Uuid, user: Uuid, role: MembershipRole) {
    sqlx::query(
        "INSERT INTO memberships (id, organization_id, user_id, role, created_at)
         VALUES ($1, $2, $3, $4, now())",
    )
    .bind(Uuid::now_v7())
    .bind(org)
    .bind(user)
    .bind(role)
    .execute(pool)
    .await
    .expect("seed membership");
}

async fn read_mfa_flag(pool: &PgPool, user_id: Uuid) -> bool {
    let (flag,): (bool,) = sqlx::query_as("SELECT mfa_enrolled FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read mfa_enrolled");
    flag
}

#[sqlx::test(migrations = "../../migrations")]
async fn insert_flips_mfa_enrolled_true(pool: PgPool) {
    let user = seed_user(&pool, "alice@example.com").await;
    assert!(
        !read_mfa_flag(&pool, user).await,
        "users.mfa_enrolled defaults to FALSE on creation",
    );

    let repo = PgPasskeyRepository::new(pool.clone());
    repo.insert(user, "cred-1", b"opaque-cbor", "macbook")
        .await
        .expect("first passkey insert");

    assert!(
        read_mfa_flag(&pool, user).await,
        "first passkey insert must flip mfa_enrolled TRUE",
    );

    // Second passkey: flag stays TRUE — UPDATE is unconditional and
    // idempotent, the property the test pins is "after a passkey
    // insert the flag is TRUE," not "the UPDATE only fires once."
    repo.insert(user, "cred-2", b"opaque-cbor-2", "phone")
        .await
        .expect("second passkey insert");
    assert!(read_mfa_flag(&pool, user).await);
}

#[sqlx::test(migrations = "../../migrations")]
async fn revoke_last_passkey_clears_mfa_enrolled(pool: PgPool) {
    let user = seed_user(&pool, "alice@example.com").await;
    let repo = PgPasskeyRepository::new(pool.clone());
    let pk1 = repo
        .insert(user, "cred-1", b"opaque-cbor", "macbook")
        .await
        .expect("first passkey");
    let pk2 = repo
        .insert(user, "cred-2", b"opaque-cbor-2", "phone")
        .await
        .expect("second passkey");

    // Revoking one of two: flag stays TRUE.
    repo.revoke(user, pk1.id).await.expect("revoke first");
    assert!(
        read_mfa_flag(&pool, user).await,
        "revoking 1 of 2 passkeys must not clear the flag",
    );

    // Revoking the last one: flag clears.
    repo.revoke(user, pk2.id).await.expect("revoke second");
    assert!(
        !read_mfa_flag(&pool, user).await,
        "revoking the last live passkey must clear mfa_enrolled",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_partial_session_owner_with_passkey_yes(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Owner).await;
    PgPasskeyRepository::new(pool.clone())
        .insert(user, "cred-1", b"opaque", "device")
        .await
        .expect("insert");

    assert!(
        session::user_requires_partial_session(&pool, user)
            .await
            .expect("query"),
        "Owner role + registered passkey ⇒ partial session",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_partial_session_admin_with_passkey_yes(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Admin).await;
    PgPasskeyRepository::new(pool.clone())
        .insert(user, "cred-1", b"opaque", "device")
        .await
        .expect("insert");

    assert!(session::user_requires_partial_session(&pool, user)
        .await
        .expect("query"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_partial_session_member_with_passkey_no(pool: PgPool) {
    // Member-role users who voluntarily register a passkey still
    // get a full session — voluntary MFA stays voluntary.
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Developer).await;
    PgPasskeyRepository::new(pool.clone())
        .insert(user, "cred-1", b"opaque", "device")
        .await
        .expect("insert");

    assert!(
        !session::user_requires_partial_session(&pool, user)
            .await
            .expect("query"),
        "Developer role with passkey ⇒ full session (voluntary MFA)",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_partial_session_owner_no_passkey_no(pool: PgPool) {
    // First-time login by a soon-to-be-promoted user: no passkey
    // yet ⇒ full session, otherwise the user could never enrol.
    // (Promotion to Owner mid-session is a separate concern; the
    // ticket scopes that to a follow-up.)
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Owner).await;

    assert!(
        !session::user_requires_partial_session(&pool, user)
            .await
            .expect("query"),
        "Owner with no passkey ⇒ full session (otherwise no enrol path)",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_partial_session_revoked_membership_no(pool: PgPool) {
    // Membership.deleted_at filters out demoted/removed users.
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Owner).await;
    PgPasskeyRepository::new(pool.clone())
        .insert(user, "cred-1", b"opaque", "device")
        .await
        .expect("insert");
    sqlx::query("UPDATE memberships SET deleted_at = now() WHERE user_id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .expect("soft-delete membership");

    assert!(
        !session::user_requires_partial_session(&pool, user)
            .await
            .expect("query"),
        "soft-deleted Owner membership ⇒ full session",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_partial_session_unknown_user_no(pool: PgPool) {
    // A user_id that doesn't exist (corrupt session, race) must
    // not crash and must not return TRUE.
    let result = session::user_requires_partial_session(&pool, Uuid::now_v7())
        .await
        .expect("query");
    assert!(!result, "unknown user_id must return false, not error");
}
