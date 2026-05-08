//! Integration tests for [`kubinate_platform::secrets::PgcryptoStore`].
//!
//! These tests exercise the real pgcrypto code path against a Postgres
//! instance. `#[sqlx::test]` creates a fresh template-derived database
//! per test and applies the migrations in `migrations/`, so tests are
//! isolated and safe to run in parallel.
//!
//! Run via `docker compose up -d` + `DATABASE_URL=... cargo test -p
//! kubinate-platform --test secrets`. In CI the harness provides both.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use kubinate_platform::secrets::{
    InMemoryVaultTransit, PgcryptoStore, SecretRef, SecretStore, VaultStore,
};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use uuid::Uuid;

/// Build a freshly-random KEK for this test process. Each test binary
/// run gets a unique key, which also acts as a guard against the stored
/// ciphertext being decryptable by a KEK from a stale test fixture.
fn fresh_kek() -> SecretString {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    SecretString::from(B64.encode(raw))
}

/// Insert an organization row so that `secrets.organization_id` has a
/// valid parent and the RLS policy can resolve.
async fn seed_org(pool: &PgPool, id: Uuid, slug: &str) {
    sqlx::query("INSERT INTO organizations (id, slug, display_name) VALUES ($1, $2, $2)")
        .bind(id)
        .bind(slug)
        .execute(pool)
        .await
        .expect("seed organization");
}

#[sqlx::test(migrations = "../../migrations")]
async fn roundtrip_preserves_plaintext(pool: PgPool) {
    let org_id = Uuid::now_v7();
    seed_org(&pool, org_id, "acme").await;

    let store = PgcryptoStore::new(pool.clone(), fresh_kek()).expect("valid KEK");

    let token = "hcloud_abc123_superSensitive_TOKEN";
    let handle = store
        .put(org_id, SecretString::from(token.to_string()))
        .await
        .expect("put");

    assert_eq!(handle.organization_id, org_id);

    let fetched = store.get(&handle).await.expect("get");
    assert_eq!(fetched.expose_secret(), token);

    store.delete(&handle).await.expect("delete");

    let after_delete = store.get(&handle).await;
    assert!(
        matches!(
            after_delete,
            Err(kubinate_platform::secrets::SecretError::NotFound(_))
        ),
        "expected NotFound after delete, got {after_delete:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn rls_blocks_cross_tenant_get(pool: PgPool) {
    let org_a = Uuid::now_v7();
    let org_b = Uuid::now_v7();
    seed_org(&pool, org_a, "acme").await;
    seed_org(&pool, org_b, "globex").await;

    let store = PgcryptoStore::new(pool.clone(), fresh_kek()).expect("valid KEK");

    let handle = store
        .put(org_a, SecretString::from("tenant-a-only".to_string()))
        .await
        .expect("put");

    // Fabricate a handle that carries the real secret id but claims
    // ownership by tenant B. RLS should prevent the lookup, surfacing
    // as `NotFound` rather than a successful decrypt.
    let forged = SecretRef {
        id: handle.id,
        organization_id: org_b,
    };

    let result = store.get(&forged).await;
    assert!(
        matches!(
            result,
            Err(kubinate_platform::secrets::SecretError::NotFound(_))
        ),
        "expected NotFound for cross-tenant access, got {result:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn wrong_kek_fails_to_decrypt(pool: PgPool) {
    let org_id = Uuid::now_v7();
    seed_org(&pool, org_id, "acme").await;

    let store = PgcryptoStore::new(pool.clone(), fresh_kek()).expect("valid KEK");
    let handle = store
        .put(org_id, SecretString::from("secret-42".to_string()))
        .await
        .expect("put");

    // A different store with a different KEK must not be able to
    // decrypt the ciphertext.
    let other_store = PgcryptoStore::new(pool.clone(), fresh_kek()).expect("valid KEK");
    let result = other_store.get(&handle).await;
    assert!(
        matches!(
            result,
            Err(kubinate_platform::secrets::SecretError::Decrypt),
        ),
        "expected Decrypt error with wrong KEK, got {result:?}",
    );
}

// ===========================================================================
// Sprint 4 ticket 02 — `VaultStore` parity tests.
//
// Same contract as the pgcrypto tests above, run against
// `VaultStore<InMemoryVaultTransit>`. The trait-level property — the
// SecretStore put/get/delete/RLS contract — must hold equivalently
// for both backends so a future migration binary can swap pgcrypto
// rows to vault rows without behaviour-level surprise.
//
// Real-Vault integration tests (live HashiCorp Vault, real
// transit-engine encrypt/decrypt) wait for Sprint 5+ when the
// dogfood deploy provides a Vault to hit.
// ===========================================================================

fn vault_store(pool: PgPool) -> VaultStore<InMemoryVaultTransit> {
    VaultStore::new(pool, InMemoryVaultTransit::new(), "kubinate-test")
}

#[sqlx::test(migrations = "../../migrations")]
async fn vault_roundtrip_preserves_plaintext(pool: PgPool) {
    let org_id = Uuid::now_v7();
    seed_org(&pool, org_id, "acme").await;

    let store = vault_store(pool.clone());

    let token = "hcloud_abc123_superSensitive_TOKEN";
    let handle = store
        .put(org_id, SecretString::from(token.to_string()))
        .await
        .expect("vault put");

    assert_eq!(handle.organization_id, org_id);

    let fetched = store.get(&handle).await.expect("vault get");
    assert_eq!(fetched.expose_secret(), token);

    store.delete(&handle).await.expect("vault delete");

    let after_delete = store.get(&handle).await;
    assert!(
        matches!(
            after_delete,
            Err(kubinate_platform::secrets::SecretError::NotFound(_))
        ),
        "expected NotFound after delete, got {after_delete:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn vault_rls_blocks_cross_tenant_get(pool: PgPool) {
    let org_a = Uuid::now_v7();
    let org_b = Uuid::now_v7();
    seed_org(&pool, org_a, "acme").await;
    seed_org(&pool, org_b, "globex").await;

    let store = vault_store(pool.clone());

    let handle = store
        .put(org_a, SecretString::from("tenant-a-only".to_string()))
        .await
        .expect("vault put");

    let forged = SecretRef {
        id: handle.id,
        organization_id: org_b,
    };
    let result = store.get(&forged).await;
    assert!(
        matches!(
            result,
            Err(kubinate_platform::secrets::SecretError::NotFound(_))
        ),
        "expected NotFound for cross-tenant access, got {result:?}",
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn vault_rejects_pgcrypto_rows(pool: PgPool) {
    // The cross-backend safety property: a row written under
    // `algorithm = pgp_sym_v1` cannot be decrypted by `VaultStore`,
    // and a row written under `vault_transit_v1` cannot be decrypted
    // by `PgcryptoStore`. This is what stops a deploy that flips
    // `KUBINATE_SECRETS_BACKEND` mid-flight from silently returning
    // wrong-shape data; the failure mode is loud (Decrypt error)
    // rather than subtle (returns wrong bytes).
    let org_id = Uuid::now_v7();
    seed_org(&pool, org_id, "acme").await;

    let pg_store = PgcryptoStore::new(pool.clone(), fresh_kek()).expect("valid KEK");
    let handle = pg_store
        .put(org_id, SecretString::from("written-by-pgcrypto".to_string()))
        .await
        .expect("pgcrypto put");

    // Now read the same row through VaultStore. The algorithm
    // column says `pgp_sym_v1`; the Vault backend must refuse
    // rather than treating the bytes as a Vault ciphertext.
    let vault = vault_store(pool.clone());
    let result = vault.get(&handle).await;
    assert!(
        matches!(
            result,
            Err(kubinate_platform::secrets::SecretError::Decrypt)
        ),
        "VaultStore must refuse pgcrypto rows; got {result:?}",
    );
}
