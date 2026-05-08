//! Integration tests for the OIDC authorization-request state store
//! (ticket 07). Exercises the single-use + TTL invariants.

use kubinate_identity::oidc::{self, AuthStateError};
use sqlx::PgPool;
use time::{Duration, OffsetDateTime};

async fn insert_state(pool: &PgPool, state: &str, expires_at: OffsetDateTime) {
    sqlx::query(
        "INSERT INTO auth_states
            (state, nonce, code_verifier, provider, redirect_to, created_at, expires_at)
         VALUES ($1, 'nonce', 'verifier', 'github', '/', now(), $2)",
    )
    .bind(state)
    .bind(expires_at)
    .execute(pool)
    .await
    .expect("seed state");
}

#[sqlx::test(migrations = "../../migrations")]
async fn store_then_consume_succeeds(pool: PgPool) {
    oidc::store(&pool, "state1", "nonce", "verifier", "github", Some("/"))
        .await
        .unwrap();
    let consumed = oidc::consume(&pool, "state1").await.unwrap();
    assert_eq!(consumed.state, "state1");
    assert_eq!(consumed.provider, "github");
    assert_eq!(consumed.code_verifier, "verifier");
}

#[sqlx::test(migrations = "../../migrations")]
async fn consume_is_single_use(pool: PgPool) {
    oidc::store(&pool, "state2", "n", "v", "github", None)
        .await
        .unwrap();
    oidc::consume(&pool, "state2").await.unwrap();
    let second = oidc::consume(&pool, "state2").await;
    assert!(
        matches!(second, Err(AuthStateError::Replay)),
        "second consume should be Replay, got {second:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn consume_expired_row_is_rejected(pool: PgPool) {
    // Bypass `store` so we can set an expiry in the past.
    let past = OffsetDateTime::now_utc() - Duration::minutes(1);
    insert_state(&pool, "state3", past).await;
    let result = oidc::consume(&pool, "state3").await;
    assert!(
        matches!(result, Err(AuthStateError::Expired)),
        "expired row should return Expired, got {result:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn consume_unknown_state_is_not_found(pool: PgPool) {
    let result = oidc::consume(&pool, "nope").await;
    assert!(
        matches!(result, Err(AuthStateError::NotFound)),
        "unknown state should return NotFound, got {result:?}",
    );
}
