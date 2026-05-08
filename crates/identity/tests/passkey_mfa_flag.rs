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

async fn read_requires_mfa(pool: &PgPool, user_id: Uuid) -> bool {
    let (flag,): (bool,) = sqlx::query_as("SELECT requires_mfa FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("read requires_mfa");
    flag
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

// ===========================================================================
// Sprint 5 ticket 07 — `users.requires_mfa` auto-flip via the
// `memberships_requires_mfa_sync` trigger. Migration
// `20260508174335_users_requires_mfa.sql`.
//
// These tests pin the trigger contract: the column tracks "does
// this user hold any live Owner/Admin membership", regardless of
// what code path mutates `memberships`. Direct SQL counts the same
// as a service-layer call.
// ===========================================================================

#[sqlx::test(migrations = "../../migrations")]
async fn requires_mfa_default_false_on_user_create(pool: PgPool) {
    let user = seed_user(&pool, "alice@example.com").await;
    assert!(
        !read_requires_mfa(&pool, user).await,
        "users.requires_mfa defaults to FALSE on user creation",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_mfa_flips_true_on_owner_membership_insert(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;

    give_role(&pool, org, user, MembershipRole::Owner).await;
    assert!(
        read_requires_mfa(&pool, user).await,
        "Owner membership insert must auto-flip requires_mfa TRUE",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_mfa_flips_true_on_admin_membership_insert(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;

    give_role(&pool, org, user, MembershipRole::Admin).await;
    assert!(read_requires_mfa(&pool, user).await);
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_mfa_stays_false_for_developer_membership(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;

    give_role(&pool, org, user, MembershipRole::Developer).await;
    assert!(
        !read_requires_mfa(&pool, user).await,
        "Developer role does not require MFA — voluntary only",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_mfa_clears_on_demotion(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Owner).await;
    assert!(read_requires_mfa(&pool, user).await);

    sqlx::query("UPDATE memberships SET role = 'developer' WHERE user_id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .expect("demote");

    assert!(
        !read_requires_mfa(&pool, user).await,
        "demoting Owner to Developer must clear requires_mfa",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_mfa_stays_true_with_one_remaining_owner_membership(pool: PgPool) {
    // Multi-org Owner: demoted in one org, still Owner in another.
    // The flag stays TRUE because the predicate is "any live
    // Owner/Admin membership across all organizations."
    let org_a = seed_org(&pool, "acme").await;
    let org_b = seed_org(&pool, "globex").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org_a, user, MembershipRole::Owner).await;
    give_role(&pool, org_b, user, MembershipRole::Admin).await;
    assert!(read_requires_mfa(&pool, user).await);

    // Demote in org A only.
    sqlx::query(
        "UPDATE memberships SET role = 'developer' WHERE user_id = $1 AND organization_id = $2",
    )
    .bind(user)
    .bind(org_a)
    .execute(&pool)
    .await
    .expect("demote in org A");

    assert!(
        read_requires_mfa(&pool, user).await,
        "remaining Admin membership in org B keeps requires_mfa TRUE",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn requires_mfa_clears_on_soft_delete(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Owner).await;
    assert!(read_requires_mfa(&pool, user).await);

    sqlx::query("UPDATE memberships SET deleted_at = now() WHERE user_id = $1")
        .bind(user)
        .execute(&pool)
        .await
        .expect("soft-delete");

    assert!(
        !read_requires_mfa(&pool, user).await,
        "soft-deleted Owner membership must clear requires_mfa",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn partial_session_for_owner_with_passkey_after_auto_flip(pool: PgPool) {
    // End-to-end: a user gets promoted to Owner via direct SQL
    // (simulating the membership service), then registers a
    // passkey. The OAuth callback's helper now reads
    // `requires_mfa AND mfa_enrolled` — both bits must be TRUE
    // after these two unrelated mutations.
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
        "Owner + passkey ⇒ partial session via the new requires_mfa AND mfa_enrolled gate",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn partial_session_skipped_for_owner_without_passkey(pool: PgPool) {
    // The whole point of decoupling requires_mfa from
    // mfa_enrolled: an Owner who hasn't enrolled yet gets a full
    // session (otherwise they can't reach /app/settings/security
    // to enrol). The SPA's mfa_state field flags "must_enrol"
    // separately; this helper stays the gate the OAuth callback
    // consults.
    let org = seed_org(&pool, "acme").await;
    let user = seed_user(&pool, "alice@example.com").await;
    give_role(&pool, org, user, MembershipRole::Owner).await;

    assert!(
        !session::user_requires_partial_session(&pool, user)
            .await
            .expect("query"),
        "Owner with no passkey ⇒ full session (must_enrol path)",
    );
}
