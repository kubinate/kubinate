//! Postgres connection pool, migrations.

use sqlx::{postgres::PgPoolOptions, PgPool};
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

/// Run migrations from the `migrations/` directory.
///
/// # Errors
/// Propagates migration errors from sqlx.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}
