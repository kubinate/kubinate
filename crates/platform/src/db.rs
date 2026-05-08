//! Postgres connection pool, migrations.

use sqlx::{postgres::PgPoolOptions, Executor, PgPool};
use std::time::Duration;

/// Build a Postgres pool from a DSN with sensible defaults.
///
/// # Errors
/// Propagates connection errors.
pub async fn pool(dsn: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(dsn)
        .await
}

/// Build a Postgres pool that issues `SET ROLE kubinate_app` on every
/// acquired connection, reusing the same connect options as `template`.
///
/// Why this exists: `#[sqlx::test]` connects as the bootstrap user
/// (a Postgres superuser in dev/CI). Superusers inherently bypass row
/// level security even when `FORCE ROW LEVEL SECURITY` is set, which
/// makes the RLS isolation tests un-runnable as-is. `SET ROLE` switches
/// `current_user` (which the RLS policy and `app_current_tenant_id()`
/// resolve against) to the non-superuser application role without
/// needing a second DSN. Production paths configure
/// `KUBINATE__DATABASE_URL` to log in directly as `kubinate_app`, so
/// this helper is test-only scaffolding — not a runtime code path.
///
/// # Errors
/// Propagates connection errors.
pub async fn app_role_pool(template: &PgPool) -> Result<PgPool, sqlx::Error> {
    let opts = (*template.connect_options()).clone();
    PgPoolOptions::new()
        .max_connections(4)
        .acquire_timeout(Duration::from_secs(5))
        .after_connect(|conn, _meta| {
            Box::pin(async move {
                conn.execute("SET ROLE kubinate_app").await?;
                Ok(())
            })
        })
        .connect_with(opts)
        .await
}

/// Run migrations from the `migrations/` directory.
///
/// # Errors
/// Propagates migration errors from sqlx.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}
