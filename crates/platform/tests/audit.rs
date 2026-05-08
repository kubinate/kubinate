//! Integration tests for the hash-chained audit log (ticket 09).
//!
//! Exercises the SQL trigger + `audit_log_verify` function by:
//!   1. Mutating `hetzner_credentials` so the trigger writes an audit row,
//!   2. Verifying the chain is intact,
//!   3. Tampering with an audit row,
//!   4. Verifying the function flags the break.

use kubinate_platform::audit;
use sqlx::PgPool;
use uuid::Uuid;

async fn seed_org(pool: &PgPool, id: Uuid, slug: &str) {
    sqlx::query("INSERT INTO organizations (id, slug, display_name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("seed org");
}

async fn seed_secret(pool: &PgPool, org_id: Uuid) -> Uuid {
    let secret_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO secrets (id, organization_id, ciphertext, wrapped_dek)
         VALUES ($1, $2, '\\x00'::bytea, '\\x00'::bytea)",
    )
    .bind(secret_id)
    .bind(org_id)
    .execute(pool)
    .await
    .expect("seed secret");
    secret_id
}

async fn insert_credential(pool: &PgPool, org_id: Uuid, alias: &str, secret_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    let mut tx = pool.begin().await.expect("begin");
    // RLS on hetzner_credentials requires app.current_tenant_id to be set.
    sqlx::query(&format!("SET LOCAL app.current_tenant_id = '{org_id}'",))
        .execute(&mut *tx)
        .await
        .expect("set tenant");
    sqlx::query(
        "INSERT INTO hetzner_credentials (id, organization_id, alias, secret_id)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(org_id)
    .bind(alias)
    .bind(secret_id)
    .execute(&mut *tx)
    .await
    .expect("insert credential");
    tx.commit().await.expect("commit");
    id
}

#[sqlx::test(migrations = "../../migrations")]
async fn trigger_appends_entries_and_chain_verifies(pool: PgPool) {
    let org = Uuid::now_v7();
    seed_org(&pool, org, "acme").await;
    let secret = seed_secret(&pool, org).await;

    let cred = insert_credential(&pool, org, "prod", secret).await;

    // Two mutations: insert above + soft-delete here. Expect two audit
    // rows with a well-formed chain.
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(&format!("SET LOCAL app.current_tenant_id = '{org}'",))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE hetzner_credentials SET deleted_at = now() WHERE id = $1")
        .bind(cred)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let count: (i64,) =
        sqlx::query_as("SELECT count(*) FROM audit_log_entries WHERE organization_id = $1")
            .bind(org)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 2, "trigger should have written two rows");

    let mut conn = pool.acquire().await.unwrap();
    let broken = audit::verify_chain(&mut conn, org).await.unwrap();
    assert!(
        broken.is_empty(),
        "chain should verify clean immediately after the trigger, got: {broken:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn verify_detects_tampered_row(pool: PgPool) {
    let org = Uuid::now_v7();
    seed_org(&pool, org, "acme").await;
    let secret = seed_secret(&pool, org).await;
    insert_credential(&pool, org, "prod", secret).await;

    // Tamper: flip `resource_id` on the single audit row. The trigger
    // is on hetzner_credentials, not on audit_log_entries, so this UPDATE
    // will not re-derive the hash. We rely on the docker-compose superuser
    // connection to bypass RLS on audit_log_entries.
    let affected = sqlx::query(
        "UPDATE audit_log_entries SET resource_id = 'tampered' WHERE organization_id = $1",
    )
    .bind(org)
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert_eq!(affected, 1);

    let mut conn = pool.acquire().await.unwrap();
    let broken = audit::verify_chain(&mut conn, org).await.unwrap();
    assert!(
        !broken.is_empty(),
        "verify should have reported a hash mismatch, got {broken:?}",
    );
    assert!(
        broken.iter().any(|(_, reason)| reason.contains("hash")),
        "expected at least one 'hash mismatch' reason, got {broken:?}",
    );
}
