//! mTLS certificate loading for the agent reverse tunnel.
//!
//! Reads three PEM files from disk and validates their format. The raw bytes
//! are stored in [`MtlsConfig`] and passed to tonic's `ClientTlsConfig` at
//! connection time.
//!
//! Required env vars (all three or none):
//!   `KUBINATE__CERT_PATH` — client cert chain (leaf + intermediates).
//!   `KUBINATE__KEY_PATH`  — private key matching the leaf cert.
//!   `KUBINATE__CA_PATH`   — CA cert used to verify the control-plane server.

#![forbid(unsafe_code)]

use anyhow::Context as _;
use rustls_pemfile::{certs, private_key};
use std::{io::BufReader, path::Path};

/// Validated mTLS material for one cluster agent session.
///
/// Fields are raw PEM bytes; tonic's `Certificate` / `Identity` wrappers
/// consume them at channel-build time.
pub struct MtlsConfig {
    pub ca_cert_pem: Vec<u8>,
    pub client_cert_pem: Vec<u8>,
    pub client_key_pem: Vec<u8>,
}

impl MtlsConfig {
    /// Read and structurally validate the three PEM files.
    ///
    /// Fails fast at startup rather than at first connection attempt so
    /// a missing or malformed cert surfaces immediately in the pod logs.
    ///
    /// # Errors
    ///
    /// Returns an error if any file cannot be read or contains no valid
    /// PEM block of the expected type.
    pub fn load(cert_path: &Path, key_path: &Path, ca_path: &Path) -> anyhow::Result<Self> {
        let client_cert_pem = std::fs::read(cert_path)
            .with_context(|| format!("read client cert {}", cert_path.display()))?;
        let client_key_pem = std::fs::read(key_path)
            .with_context(|| format!("read client key {}", key_path.display()))?;
        let ca_cert_pem = std::fs::read(ca_path)
            .with_context(|| format!("read CA cert {}", ca_path.display()))?;

        // Validate each file so we surface format errors at startup.
        validate_cert_pem(&client_cert_pem).context("client cert PEM invalid")?;
        validate_key_pem(&client_key_pem).context("client key PEM invalid")?;
        validate_cert_pem(&ca_cert_pem).context("CA cert PEM invalid")?;

        Ok(Self {
            ca_cert_pem,
            client_cert_pem,
            client_key_pem,
        })
    }
}

fn validate_cert_pem(pem: &[u8]) -> anyhow::Result<()> {
    let mut reader = BufReader::new(pem);
    let count = certs(&mut reader).count();
    anyhow::ensure!(count > 0, "no certificate blocks found");
    Ok(())
}

fn validate_key_pem(pem: &[u8]) -> anyhow::Result<()> {
    let mut reader = BufReader::new(pem);
    private_key(&mut reader)
        .context("failed to parse private key")?
        .context("no private key block found")?;
    Ok(())
}
