//! Sprint 3 ticket 06 — column-aware audit trigger on `organizations`.
//!
//! Confirms the AC:
//!   1. `UPDATE plan` writes one chain-valid audit row.
//!   2. `UPDATE display_name` writes nothing.
//!   3. The audit row's metadata carries before/after values.
//!   4. `audit_log_verify` is clean across both flows.

use kubinate_platform::audit;
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

async fn audit_count_for(pool: &PgPool, org: Uuid) -> i64 {
    let row: (i64,) = sqlx::query_as(
        "SELECT count(*) FROM audit_log_entries
         WHERE organization_id = $1 AND resource_type = 'organizations'",
    )
    .bind(org)
    .fetch_one(pool)
    .await
    .unwrap();
    row.0
}

#[sqlx::test(migrations = "../../migrations")]
async fn plan_change_writes_audit_row(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    assert_eq!(
        audit_count_for(&pool, org).await,
        0,
        "no audit row at insert"
    );

    sqlx::query("UPDATE organizations SET plan = 'starter' WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await
        .expect("plan update");

    assert_eq!(
        audit_count_for(&pool, org).await,
        1,
        "plan UPDATE must write exactly one audit row",
    );

    let row: (String, serde_json::Value) = sqlx::query_as(
        "SELECT action, metadata FROM audit_log_entries
         WHERE organization_id = $1
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "organizations.updated");
    assert_eq!(row.1["old_plan"], "free");
    assert_eq!(row.1["new_plan"], "starter");

    // Chain still verifies.
    let mut conn = pool.acquire().await.unwrap();
    let broken = audit::verify_chain(&mut conn, org).await.unwrap();
    assert!(
        broken.is_empty(),
        "chain must verify after plan change: {broken:?}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn non_billing_update_does_not_write_audit(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    sqlx::query("UPDATE organizations SET display_name = 'New Name' WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await
        .expect("display name update");

    assert_eq!(
        audit_count_for(&pool, org).await,
        0,
        "non-billing column updates must not produce audit rows",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn idempotent_plan_set_does_not_emit(pool: PgPool) {
    // `SET plan = plan` puts the column in the trigger's WHEN-list,
    // but our IS DISTINCT FROM short-circuit must skip the insert.
    let org = seed_org(&pool, "acme").await;
    sqlx::query("UPDATE organizations SET plan = plan WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await
        .expect("idempotent plan update");

    assert_eq!(
        audit_count_for(&pool, org).await,
        0,
        "no-op plan update must not write an audit row",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn stripe_customer_link_writes_audit_row(pool: PgPool) {
    let org = seed_org(&pool, "acme").await;
    sqlx::query("UPDATE organizations SET stripe_customer_id = 'cus_123' WHERE id = $1")
        .bind(org)
        .execute(&pool)
        .await
        .expect("link customer");
    let row: (serde_json::Value,) = sqlx::query_as(
        "SELECT metadata FROM audit_log_entries
         WHERE organization_id = $1 ORDER BY created_at DESC LIMIT 1",
    )
    .bind(org)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0["old_stripe_customer_id"], serde_json::Value::Null);
    assert_eq!(row.0["new_stripe_customer_id"], "cus_123");
}
