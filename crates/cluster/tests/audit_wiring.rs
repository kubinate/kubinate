//! Verifies that the runner-shape `AuditContext` (no actor, but a
//! workflow-id `request_id` and `runner/*` user agent) lands on audit
//! rows produced by background-task mutations (Sprint 2 ticket 01 AC #2).

use kubinate_cluster::{
    model::{ClusterStatus, NewCluster},
    repository::{ClusterRepository, PgClusterRepository},
};
use kubinate_platform::audit::AuditContext;
use sqlx::PgPool;
use uuid::Uuid;

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

async fn seed_credential(pool: &PgPool, org: Uuid) -> Uuid {
    // hetzner_credentials → secrets FK; we need a secrets row first.
    let secret_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO secrets (id, organization_id, ciphertext, wrapped_dek)
         VALUES ($1, $2, '\\x00'::bytea, '\\x00'::bytea)",
    )
    .bind(secret_id)
    .bind(org)
    .execute(pool)
    .await
    .expect("seed secret");

    let cred_id = Uuid::now_v7();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query(&format!("SET LOCAL app.current_tenant_id = '{org}'"))
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO hetzner_credentials (id, organization_id, alias, secret_id)
         VALUES ($1, $2, 'prod', $3)",
    )
    .bind(cred_id)
    .bind(org)
    .bind(secret_id)
    .execute(&mut *tx)
    .await
    .expect("seed credential");
    tx.commit().await.unwrap();
    cred_id
}

#[sqlx::test(migrations = "../../migrations")]
async fn runner_shape_audit_writes_null_actor_and_workflow_request_id(pool: PgPool) {
    let org = seed_org(&pool).await;
    let credential_id = seed_credential(&pool, org).await;

    let repo = PgClusterRepository::new(pool.clone());

    // Create a cluster with an interactive-looking audit context so
    // the test focuses on what's different about the runner path.
    let api_audit = AuditContext {
        actor_user_id: None,
        request_id: Some("seed".into()),
        ip: None,
        user_agent: Some("test/seed".into()),
    };
    let cluster = repo
        .insert(
            org,
            NewCluster {
                name: "prod".into(),
                region: "nbg1".into(),
                server_type: "cpx21".into(),
                control_plane_count: 1,
                worker_count: 1,
                credential_id,
            },
            &api_audit,
        )
        .await
        .expect("insert cluster");

    // Now simulate the runner: no actor, request_id is the workflow
    // uuid, user_agent identifies the runner.
    let workflow_id = Uuid::now_v7();
    let runner_audit = AuditContext {
        actor_user_id: None,
        request_id: Some(workflow_id.to_string()),
        ip: None,
        user_agent: Some("runner/provision".into()),
    };
    repo.update_status(
        org,
        cluster.id,
        ClusterStatus::Provisioning,
        None,
        &runner_audit,
    )
    .await
    .expect("update_status");

    // The most recent audit row for this cluster should be the
    // runner-driven UPDATE (action = clusters.updated).
    let row: (Option<Uuid>, Option<String>, Option<String>, String) = sqlx::query_as(
        "SELECT actor_user_id, request_id, user_agent, action
         FROM audit_log_entries
         WHERE organization_id = $1 AND resource_id = $2
         ORDER BY created_at DESC, id DESC
         LIMIT 1",
    )
    .bind(org)
    .bind(cluster.id.to_string())
    .fetch_one(&pool)
    .await
    .expect("audit row exists");

    assert_eq!(row.0, None, "runner path must leave actor_user_id NULL");
    assert_eq!(row.1.as_deref(), Some(workflow_id.to_string().as_str()));
    assert_eq!(row.2.as_deref(), Some("runner/provision"));
    assert_eq!(row.3, "clusters.updated");
}
