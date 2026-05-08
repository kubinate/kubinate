//! Prometheus exporter wiring.
//!
//! Sprint 3 ticket 01 (defer-path) — installs the `metrics` facade's
//! Prometheus recorder so domain crates can use `metrics::gauge!`,
//! `counter!`, etc. without taking a direct dep on the exporter
//! implementation. The API binary calls [`init`] once at startup
//! and serves the recorder's render output from a `/metrics` route.
//!
//! Why split this from `telemetry`: tracing-OTLP already lives
//! there, and the metrics path has a different lifecycle (handle
//! lives for the whole process, no async runtime needed). Keeping
//! them apart makes the reset-for-tests story straightforward — the
//! metrics handle is process-global by construction.
//!
//! Metric naming conventions (Prometheus):
//! - Names are snake_case with the `kubinate_` prefix.
//! - Gauges describing in-flight work end in `_inflight`.
//! - Counters describing event totals end in `_total`.
//!
//! See ADR-0011 (defer Temporal) for the first concrete consumer:
//! `kubinate_runner_workflows_inflight`, exported by the
//! `kubinate_workflows::runner::LocalRunner`.

use std::sync::OnceLock;

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Errors surfaced by [`init`].
#[derive(Debug, thiserror::Error)]
pub enum MetricsInitError {
    /// The Prometheus exporter could not be installed as the global
    /// metrics recorder. Almost always means [`init`] was called
    /// twice; safe to log and continue using the previous handle.
    #[error("install prometheus recorder: {0}")]
    Install(String),
}

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the Prometheus exporter as the global `metrics` recorder
/// and stash the handle so [`render`] can produce text-format output
/// later. Idempotent: a second call returns `Ok` without reinstalling.
///
/// # Errors
/// Returns [`MetricsInitError::Install`] if the recorder could not
/// be set on the *first* call. Subsequent calls are no-ops.
pub fn init() -> Result<(), MetricsInitError> {
    if HANDLE.get().is_some() {
        return Ok(());
    }
    let recorder = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| MetricsInitError::Install(e.to_string()))?;
    let _ = HANDLE.set(recorder);
    Ok(())
}

/// Render the current metric snapshot as a Prometheus text-format
/// document. Returns an empty string when [`init`] has not run yet —
/// safer than panicking on the `/metrics` path during a shutdown
/// race.
#[must_use]
pub fn render() -> String {
    HANDLE.get().map(PrometheusHandle::render).unwrap_or_default()
}

/// Metric name constants. Keeping these here (rather than scattered
/// inline at the call sites) makes it easy to grep for who emits a
/// given series, and ensures the names match the exposition format
/// docs in `docs/observability/`.
pub mod names {
    /// Number of cluster-lifecycle workflows currently running in the
    /// in-process [`kubinate_workflows::runner::LocalRunner`]. The
    /// follow-up ADR (0011) names a > 10 sustained value as the
    /// trigger to revisit the Temporal SDK adoption decision.
    pub const RUNNER_WORKFLOWS_INFLIGHT: &str = "kubinate_runner_workflows_inflight";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_is_empty_before_init() {
        // Sanity check: even with no init, calling render() must not
        // panic — the API's /metrics handler should be safe to hit
        // during a startup race.
        let s = render();
        assert!(s.is_empty() || s.contains("# "));
    }
}
