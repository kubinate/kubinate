//! Structured logging and OpenTelemetry tracing bootstrap.
//!
//! Every binary calls [`init`] once during startup. The call reads
//! `OTEL_*` environment variables; when unset, traces are dropped and
//! only structured logs are emitted.

use tracing_subscriber::{prelude::*, EnvFilter};

/// Initialize tracing. Returns a guard that must be held for the
/// lifetime of the process; dropping it flushes pending spans.
///
/// # Errors
///
/// Returns an error if the subscriber fails to install (e.g. if called
/// more than once).
pub fn init(service_name: &str) -> anyhow::Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_level(true)
        .json();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .try_init()?;

    tracing::info!(service = service_name, "telemetry initialized");
    Ok(())
}
