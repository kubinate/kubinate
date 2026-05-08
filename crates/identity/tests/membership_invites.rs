//! Sprint 2 ticket 06 — invite + membership integration tests.
//!
//! Exercises the full happy path through `MembershipService`:
//!   * owner creates an invite,
//!   * invitee (post-OIDC) accepts using the raw token,
//!   * a `memberships` row exists with the requested role.
//!
//! Plus the security AC: an invite for org A cannot be accepted by a
//! user whose email belongs to a different invite or who is targeting
//! a different organization.

use std::sync::Arc;

use kubinate_identity::{
    model::MembershipRole,
    repository::{
        InviteRepository, MembershipRepository, PgInviteRepository, PgMembershipRepository,
    },
    service::{InviteError, MembershipService},
};
use kubinate_platform::audit::AuditContext;
use secrecy::SecretString;
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

async fn give_role(repo: &dyn MembershipRepository, org: Uuid, user: Uuid, role: MembershipRole) {
    let audit = AuditContext {
        actor_user_id: None,
        request_id: Some("seed".into()),
        ip: None,
        user_agent: Some("test/seed".into()),
    };
    repo.insert(org, user, role, &audit)
        .await
        .expect("seed membership");
}

fn build_service(pool: &PgPool) -> (MembershipService, Arc<dyn MembershipRepository>) {
    let memberships: Arc<dyn MembershipRepository> =
        Arc::new(PgMembershipRepository::new(pool.clone()));
    let invites: Arc<dyn InviteRepository> = Arc::new(PgInviteRepository::new(pool.clone()));
    let svc = MembershipService::new(invites, memberships.clone());
    (svc, memberships)
}

#[sqlx::test(migrations = "../../migrations")]
async fn happy_path_create_invite_then_accept_makes_membership(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let owner = seed_user(&pool, "owner@acme.test").await;
    let invitee = seed_user(&pool, "newhire@acme.test").await;

    let (service, memberships) = build_service(&pool);
    give_role(&*memberships, org, owner, MembershipRole::Owner).await;

    let audit = AuditContext {
        actor_user_id: Some(owner),
        request_id: Some("req-1".into()),
        ip: None,
        user_agent: Some("test/api".into()),
    };

    let created = service
        .create_invite(
            org,
            owner,
            "NewHire@acme.test".into(),
            MembershipRole::Developer,
            &audit,
        )
        .await
        .expect("create invite");

    // Token is `kinv_<base64>`; the prefix ends up on the invite row.
    assert!(created.invite.token_prefix.starts_with("kinv_"));

    let accepted = service
        .accept_invite(
            SecretString::from(created.token),
            invitee,
            "newhire@acme.test", // case-insensitive match
            &audit,
        )
        .await
        .expect("accept invite");
    assert_eq!(accepted.organization_id, org);

    // A membership row now exists with the developer role.
    let role = memberships
        .role_of(org, invitee)
        .await
        .expect("role lookup")
        .expect("membership row");
    assert_eq!(role, MembershipRole::Developer);

    // The invite is no longer live.
    let pending = service.list_invites(org, owner).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].accepted_at.is_some());
}

#[sqlx::test(migrations = "../../migrations")]
async fn accept_rejects_email_mismatch(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let owner = seed_user(&pool, "owner@acme.test").await;
    let intended = seed_user(&pool, "intended@acme.test").await;
    let _ = intended;
    let attacker = seed_user(&pool, "attacker@evil.test").await;

    let (service, memberships) = build_service(&pool);
    give_role(&*memberships, org, owner, MembershipRole::Admin).await;

    let audit = AuditContext {
        actor_user_id: Some(owner),
        request_id: Some("req-2".into()),
        ip: None,
        user_agent: Some("test/api".into()),
    };
    let created = service
        .create_invite(
            org,
            owner,
            "intended@acme.test".into(),
            MembershipRole::Viewer,
            &audit,
        )
        .await
        .unwrap();

    let result = service
        .accept_invite(
            SecretString::from(created.token),
            attacker,
            "attacker@evil.test",
            &audit,
        )
        .await;
    assert!(
        matches!(result, Err(InviteError::Forbidden(_))),
        "wrong-email accept must be Forbidden, got {result:?}",
    );
    // No membership leaked into the org.
    assert!(memberships.role_of(org, attacker).await.unwrap().is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn non_owner_cannot_invite(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let dev = seed_user(&pool, "dev@acme.test").await;
    let (service, memberships) = build_service(&pool);
    give_role(&*memberships, org, dev, MembershipRole::Developer).await;

    let audit = AuditContext {
        actor_user_id: Some(dev),
        request_id: Some("req-3".into()),
        ip: None,
        user_agent: Some("test/api".into()),
    };
    let err = service
        .create_invite(
            org,
            dev,
            "someone@acme.test".into(),
            MembershipRole::Viewer,
            &audit,
        )
        .await
        .expect_err("developer must not be able to invite");
    assert!(matches!(err, InviteError::Forbidden(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn cannot_demote_last_owner(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    let owner = seed_user(&pool, "owner@acme.test").await;
    let (service, memberships) = build_service(&pool);
    give_role(&*memberships, org, owner, MembershipRole::Owner).await;

    let audit = AuditContext {
        actor_user_id: Some(owner),
        request_id: Some("req-4".into()),
        ip: None,
        user_agent: Some("test/api".into()),
    };
    let err = service
        .update_role(org, owner, MembershipRole::Admin, owner, &audit)
        .await
        .expect_err("demoting last owner must fail");
    assert!(matches!(err, InviteError::Forbidden(_)));

    // Still owner.
    assert_eq!(
        memberships.role_of(org, owner).await.unwrap(),
        Some(MembershipRole::Owner)
    );
}
