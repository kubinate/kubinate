//! Tenant-scoped transaction helper.
//!
//! Every request that touches tenant data must open the database
//! transaction through [`TenantScopedTransaction`]. It sets the
//! Postgres session variable `app.current_tenant_id` via `SET LOCAL`,
//! which is the key input to the row-level-security policies defined
//! in migrations. See ADR-0006.

use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::PlatformError;

/// Wraps a Postgres transaction and ensures that the tenant
/// discriminator is set on the session before any query runs.
pub struct TenantScopedTransaction<'a> {
    tx: Transaction<'a, Postgres>,
}

impl<'a> TenantScopedTransaction<'a> {
    /// Open a new tenant-scoped transaction.
    ///
    /// # Errors
    /// Propagates database errors from `BEGIN` or the `SET LOCAL` call.
    pub async fn begin(pool: &'a PgPool, organization_id: Uuid) -> Result<Self, PlatformError> {
        let mut tx = pool.begin().await?;
        // SET LOCAL is scoped to the current transaction and is reset
        // on COMMIT/ROLLBACK. Using a parameterized form via format!
        // is acceptable here because the Uuid Display impl is
        // hex-only and cannot produce SQL injection.
        let stmt = format!("SET LOCAL app.current_tenant_id = '{organization_id}'");
        sqlx::query(&stmt).execute(&mut *tx).await?;
        Ok(Self { tx })
    }

    /// Borrow the underlying executor for queries.
    pub fn executor(&mut self) -> &mut Transaction<'a, Postgres> {
        &mut self.tx
    }

    /// Commit the transaction.
    ///
    /// # Errors
    /// Propagates database errors.
    pub async fn commit(self) -> Result<(), PlatformError> {
        self.tx.commit().await?;
        Ok(())
    }
}
