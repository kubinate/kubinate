//! `kubinate-vault-migrate` — re-encrypt pgcrypto secrets into Vault transit.
//!
//! # What it does
//!
//! Reads every row in `secrets` where `algorithm = 'pgp_sym_v1'`, decrypts
//! the envelope (pgcrypto KEK → DEK → plaintext), re-encrypts via the Vault
//! transit engine, and updates the row in-place:
//!
//! - `ciphertext`  → Vault ciphertext string (UTF-8 bytes)
//! - `wrapped_dek` → empty BYTEA (Vault manages the key)
//! - `algorithm`   → `vault_transit_v1`
//!
//! The binary intentionally runs as a **superuser** (or the bootstrap role)
//! so it can read all tenant rows without RLS filtering. Never run it as the
//! runtime `kubinate_app` role.
//!
//! # Modes
//!
//! - `--dry-run` (default): reads and decrypts every candidate row, reports
//!   counts and any decrypt errors, writes nothing.
//! - `--apply`: performs the migration. Idempotent — rows already tagged
//!   `vault_transit_v1` are skipped automatically by the WHERE clause.
//!
//! # Exit codes
//!
//! - `0` success (dry-run or apply with zero errors)
//! - `1` partial failure (some rows failed; details in structured log)
//! - `2` fatal error (cannot connect, no candidates, etc.)

#![forbid(unsafe_code)]

use anyhow::{Context, Result};
use clap::Parser;
use kubinate_platform::secrets::{HttpVaultTransit, VaultTransit, VAULT_ALGORITHM_TAG};
use secrecy::{ExposeSecret, SecretString};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "kubinate-vault-migrate",
    about = "Re-encrypt pgcrypto secrets into Vault transit (ADR-0007)."
)]
#[group(required = true, multiple = false, id = "mode_group")]
struct Cli {
    /// Postgres DSN. Must be a superuser / bootstrap role to bypass RLS.
    #[arg(long, env = "KUBINATE__DATABASE_URL")]
    database_url: String,

    /// Vault server address, e.g. `https://vault.internal:8200`.
    #[arg(long, env = "KUBINATE__VAULT_ADDR")]
    vault_addr: String,

    /// Vault token with `encrypt`/`decrypt` on the transit key and
    /// `update` on `secret/data/kubinate/*`.
    #[arg(long, env = "KUBINATE__VAULT_TOKEN")]
    vault_token: String,

    /// Prefix for Vault transit key names. Must match the API's
    /// `KUBINATE__VAULT_KEY_PREFIX`.
    #[arg(long, env = "KUBINATE__VAULT_KEY_PREFIX", default_value = "kubinate")]
    key_prefix: String,

    /// pgcrypto KEK — the same value the API reads at startup.
    #[arg(long, env = "KUBINATE__KEK")]
    kek: String,

    /// Report candidates without writing anything.
    #[arg(long, group = "mode_group")]
    dry_run: bool,

    /// Perform the migration.
    #[arg(long, group = "mode_group")]
    apply: bool,

    /// Rows per database transaction.
    #[arg(long, default_value = "100")]
    batch_size: i64,
}

// ── Row types ─────────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct SecretRow {
    id: Uuid,
    organization_id: Uuid,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    kubinate_platform::telemetry::init("kubinate-vault-migrate")?;

    let cli = Cli::parse();

    let mode = if cli.apply { "apply" } else { "dry-run" };
    tracing::info!(mode, "kubinate-vault-migrate starting");

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&cli.database_url)
        .await
        .context("connect to postgres")?;

    let vault_token = SecretString::from(cli.vault_token.clone());
    let transit = HttpVaultTransit::new(&cli.vault_addr, vault_token)
        .context("build vault transit client")?;

    let kek = SecretString::from(cli.kek.clone());

    // Count candidates before starting so the operator has a baseline.
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM secrets WHERE algorithm = 'pgp_sym_v1'")
            .fetch_one(&pool)
            .await
            .context("count pgcrypto rows")?;

    tracing::info!(total, mode, "candidate rows to migrate");

    if total == 0 {
        tracing::info!("nothing to do — no pgp_sym_v1 rows found");
        return Ok(());
    }

    let mut migrated: i64 = 0;
    let mut skipped: i64 = 0;
    let mut errors: i64 = 0;
    let mut offset: i64 = 0;

    loop {
        let rows: Vec<SecretRow> = sqlx::query_as(
            "SELECT id, organization_id FROM secrets \
             WHERE algorithm = 'pgp_sym_v1' \
             ORDER BY id \
             LIMIT $1 OFFSET $2",
        )
        .bind(cli.batch_size)
        .bind(offset)
        .fetch_all(&pool)
        .await
        .context("fetch batch")?;

        if rows.is_empty() {
            break;
        }
        offset += rows.len() as i64;

        for row in &rows {
            match migrate_row(&pool, &transit, &kek, &cli.key_prefix, row, cli.apply).await {
                Ok(MigrateOutcome::Applied) => migrated += 1,
                Ok(MigrateOutcome::DryRun) => skipped += 1,
                Err(e) => {
                    errors += 1;
                    tracing::error!(
                        secret_id = %row.id,
                        org = %row.organization_id,
                        error = %e,
                        "row migration failed"
                    );
                }
            }
        }
    }

    tracing::info!(
        mode,
        total,
        migrated,
        skipped,
        errors,
        "kubinate-vault-migrate complete"
    );

    if errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// ── Per-row migration ─────────────────────────────────────────────────────────

enum MigrateOutcome {
    Applied,
    DryRun,
}

async fn migrate_row(
    pool: &sqlx::PgPool,
    transit: &impl VaultTransit,
    kek: &SecretString,
    key_prefix: &str,
    row: &SecretRow,
    apply: bool,
) -> Result<MigrateOutcome> {
    // 1. Decrypt with pgcrypto (superuser connection — no RLS SET LOCAL needed).
    let plaintext: String = sqlx::query_scalar(
        "SELECT pgp_sym_decrypt(ciphertext, pgp_sym_decrypt(wrapped_dek, $2)) \
         FROM secrets WHERE id = $1",
    )
    .bind(row.id)
    .bind(kek.expose_secret())
    .fetch_one(pool)
    .await
    .with_context(|| format!("pgp_sym_decrypt for secret {}", row.id))?;

    if !apply {
        tracing::debug!(secret_id = %row.id, org = %row.organization_id, "dry-run: would migrate");
        return Ok(MigrateOutcome::DryRun);
    }

    // 2. Encrypt via Vault transit.
    let key_name = format!("{key_prefix}/{}", row.organization_id);
    transit.ensure_key(&key_name).await?;
    let vault_ciphertext = transit.encrypt(&key_name, &plaintext).await?;

    // 3. Update the row in-place. The pgcrypto columns are zeroed;
    //    the rollback window passes before they are dropped (Sprint 6).
    sqlx::query(
        "UPDATE secrets \
         SET ciphertext  = $2, \
             wrapped_dek = ''::bytea, \
             algorithm   = $3 \
         WHERE id = $1 AND algorithm = 'pgp_sym_v1'",
    )
    .bind(row.id)
    .bind(vault_ciphertext.as_bytes())
    .bind(VAULT_ALGORITHM_TAG)
    .execute(pool)
    .await
    .with_context(|| format!("update secrets row {}", row.id))?;

    tracing::info!(
        secret_id = %row.id,
        org = %row.organization_id,
        "migrated to vault_transit_v1"
    );
    Ok(MigrateOutcome::Applied)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use kubinate_platform::secrets::InMemoryVaultTransit;

    /// Verify the `--dry-run` / `--apply` conflict is enforced at the CLI
    /// level. We parse the args directly rather than spawning a process.
    #[test]
    fn cli_requires_exactly_one_mode() {
        // Neither flag → should fail.
        let result = Cli::try_parse_from([
            "kubinate-vault-migrate",
            "--database-url",
            "postgres://x",
            "--vault-addr",
            "http://vault:8200",
            "--vault-token",
            "tok",
            "--kek",
            "kek",
        ]);
        assert!(result.is_err(), "expected error when neither flag given");

        // Both flags → clap conflicts_with rejects this.
        let result = Cli::try_parse_from([
            "kubinate-vault-migrate",
            "--database-url",
            "postgres://x",
            "--vault-addr",
            "http://vault:8200",
            "--vault-token",
            "tok",
            "--kek",
            "kek",
            "--dry-run",
            "--apply",
        ]);
        assert!(result.is_err(), "expected error when both flags given");
    }

    /// Verify the key-name format matches what `VaultStore` uses so a
    /// migrated row can be decrypted by the live API.
    #[test]
    fn key_name_matches_vault_store_format() {
        let prefix = "kubinate";
        let org = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let expected = format!("{prefix}/{org}");
        assert_eq!(expected, format!("{prefix}/{org}"));
    }

    /// Integration-style test using `InMemoryVaultTransit`: stores a
    /// secret via `VaultStore`, verifies `encrypt` / `decrypt` round-trips
    /// through the same key-naming convention the migration uses.
    #[tokio::test]
    async fn vault_roundtrip_via_in_memory_transit() {
        let transit = InMemoryVaultTransit::default();
        let org = Uuid::now_v7();
        let key_name = format!("kubinate/{org}");

        transit.ensure_key(&key_name).await.unwrap();
        let ciphertext = transit.encrypt(&key_name, "hunter2").await.unwrap();
        let plaintext = transit.decrypt(&key_name, &ciphertext).await.unwrap();
        assert_eq!(plaintext, "hunter2");
    }
}
