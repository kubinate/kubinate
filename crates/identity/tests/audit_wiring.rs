//! Verifies that the `AuditContext` plumbed through the credential
//! service ends up in the audit-log row written by the trigger
//! (Sprint 2 ticket 01 AC #1).

use std::sync::Arc;

use kubinate_identity::{
    repository::PgHetznerCredentialRepository, service::HetznerCredentialService,
};
use kubinate_platform::{
    audit::AuditContext,
    secrets::{PgcryptoStore, SecretStore},
};
use rand::RngCore;
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

fn fresh_kek() -> SecretString {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    SecretString::from(B64.encode(raw))
}

async fn seed_org(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO organizations (id, slug, display_name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(format!("acme-{}", &id.simple().to_string()[..8]))
        .execute(pool)
        .await
        .expect("seed org");
    id
}

async fn seed_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, 'Alice')")
        .bind(id)
        .bind(format!("alice+{id}@example.test"))
        .execute(pool)
        .await
        .expect("seed user");
    id
}

#[sqlx::test(migrations = "../../migrations")]
async fn api_audit_context_lands_on_credential_create(pool: PgPool) {
    let org = seed_org(&pool).await;
    let user = seed_user(&pool).await;

    let store: Arc<dyn SecretStore> =
        Arc::new(PgcryptoStore::new(pool.clone(), fresh_kek()).expect("kek"));
    let repo = Arc::new(PgHetznerCredentialRepository::new(pool.clone()));
    let service = HetznerCredentialService::new(repo, store);

    let req_id = "req-9b9c-001".to_string();
    let audit = AuditContext {
        actor_user_id: Some(user),
        request_id: Some(req_id.clone()),
        ip: None,
        user_agent: Some("test/api".to_string()),
    };

    let credential = service
        .create(org, "prod".into(), SecretString::from("hcloud_x"), &audit)
        .await
        .expect("create");

    // The trigger writes one row per DML; for a fresh INSERT we expect
    // exactly one entry tagged with this credential's id.
    let row: (Option<Uuid>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT actor_user_id, request_id, user_agent
         FROM audit_log_entries
         WHERE organization_id = $1 AND resource_id = $2",
    )
    .bind(org)
    .bind(credential.id.to_string())
    .fetch_one(&pool)
    .await
    .expect("audit row exists");

    assert_eq!(
        row.0,
        Some(user),
        "actor_user_id should match the API actor"
    );
    assert_eq!(row.1.as_deref(), Some(req_id.as_str()));
    assert_eq!(row.2.as_deref(), Some("test/api"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn dev_header_actor_with_nil_user_writes_null_actor(pool: PgPool) {
    let org = seed_org(&pool).await;

    let store: Arc<dyn SecretStore> =
        Arc::new(PgcryptoStore::new(pool.clone(), fresh_kek()).expect("kek"));
    let repo = Arc::new(PgHetznerCredentialRepository::new(pool.clone()));
    let service = HetznerCredentialService::new(repo, store);

    // Mirror what `audit_ctx::from_actor_and_headers` does for the
    // dev-header path with no `X-Actor-User-Id`: nil → None, never the
    // zero UUID (Sprint 2 ticket 01 AC #3).
    let audit = AuditContext {
        actor_user_id: None,
        request_id: Some("req-dev-001".into()),
        ip: None,
        user_agent: Some("dev/header".to_string()),
    };

    let credential = service
        .create(org, "dev".into(), SecretString::from("hcloud_x"), &audit)
        .await
        .expect("create");

    let row: (Option<Uuid>, Option<String>) = sqlx::query_as(
        "SELECT actor_user_id, request_id FROM audit_log_entries
         WHERE organization_id = $1 AND resource_id = $2",
    )
    .bind(org)
    .bind(credential.id.to_string())
    .fetch_one(&pool)
    .await
    .expect("audit row exists");

    assert_eq!(
        row.0, None,
        "actor_user_id must stay NULL when the dev path has no user, not nil UUID",
    );
    assert_eq!(row.1.as_deref(), Some("req-dev-001"));
}
