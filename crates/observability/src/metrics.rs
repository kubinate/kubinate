//! Metric ingest + tenant-scoped query for Sprint 3 ticket 07.
//!
//! Wire format: JSON. The production target is Prometheus
//! remote-write protobuf; we'll swap when the agent reverse tunnel
//! lands and the backend (`VictoriaMetrics` in the default rec; see
//! `docs/decisions/sprint-3-observability-tsdb.md`) is wired.
//!
//! The shape here is the smallest thing that exercises the
//! tenant-isolation invariant the spike's M2 measurement cares
//! about:
//!
//! - Every sample is keyed by `organization_id` at the boundary.
//!   The store *cannot* return a sample for a tenant that didn't
//!   write it, regardless of what labels the sample carries.
//! - Queries take an `organization_id` argument. There is no API
//!   that returns "all" samples — even an admin-style listing
//!   would have to enumerate tenants by id.
//! - The store is behind a trait so the eventual live
//!   `VictoriaMetricsStore` lands without changing the API or the
//!   tenant-isolation tests.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Errors surfaced by [`MetricsStore`].
#[derive(Debug, Error)]
pub enum MetricsError {
    /// The caller's authenticated tenant does not match the tenant
    /// id claimed in the ingest body. This is the headline thing the
    /// proxy enforces; bubble it up so the API layer can return 403
    /// instead of silently re-keying.
    #[error("tenant mismatch: actor=<{actor}> body=<{body}>")]
    TenantMismatch {
        /// Tenant id from the authenticated session.
        actor: Uuid,
        /// Tenant id claimed in the ingest body.
        body: Uuid,
    },
    /// Internal storage failure (would never happen for the in-memory
    /// store; reserved for the live VM-backed impl).
    #[error("backend: {0}")]
    Backend(#[from] anyhow::Error),
}

/// One metric label pair. `name` is the Prometheus identifier
/// (`__name__` for the metric name itself), `value` the label
/// value. Multi-tenancy is enforced *outside* the labels — the
/// `organization_id` never appears here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    /// Label name. Conventionally `lower_snake`; `__name__` for the
    /// series name itself.
    pub name: String,
    /// Label value. Free-form UTF-8.
    pub value: String,
}

/// One sample point. Mirrors the shape Prometheus remote-write uses
/// per-series — labels (1) plus zero-or-more (timestamp, value)
/// pairs (we keep one pair per sample to keep the in-memory store
/// trivial; remote-write batches these).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    /// Identifying labels for the series this sample belongs to.
    pub labels: Vec<Label>,
    /// Unix milliseconds since epoch.
    pub timestamp_ms: i64,
    /// Sample value. Prometheus is float64; we follow.
    pub value: f64,
}

/// Wire format for the JSON ingest endpoint. Mirrors a single
/// remote-write `WriteRequest` minus the protobuf encoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteRequest {
    /// Tenant the writer is asserting the samples belong to.
    /// Cross-checked against the authenticated actor's tenant.
    pub organization_id: Uuid,
    /// Samples in this batch.
    pub samples: Vec<Sample>,
}

/// Range query. The matcher set is intentionally minimal in the
/// scaffold — equality on `__name__` plus zero-or-more equality
/// matchers on labels. The live VM-backed store will accept the
/// full `PromQL` matcher set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeQuery {
    /// Required `__name__` label value.
    pub metric: String,
    /// Equality matchers on labels other than `__name__`.
    #[serde(default)]
    pub label_eq: Vec<Label>,
    /// Inclusive lower bound (Unix ms).
    pub start_ms: i64,
    /// Inclusive upper bound (Unix ms).
    pub end_ms: i64,
}

/// Backend store for metrics. Domain code holds an
/// `Arc<dyn MetricsStore>`; tests use [`InMemoryMetricsStore`], the
/// production deploy will use `VictoriaMetricsStore` (later
/// ticket).
#[async_trait]
pub trait MetricsStore: Send + Sync {
    /// Ingest a batch of samples for a tenant. Returns the number
    /// of samples accepted (pre-throttling, pre-deduplication; for
    /// the scaffold this equals `samples.len()`).
    async fn ingest(
        &self,
        organization_id: Uuid,
        samples: Vec<Sample>,
    ) -> Result<usize, MetricsError>;

    /// Range query, scoped to a single tenant. Returning an empty
    /// `Vec` is the only legal response to "this tenant has no
    /// matching samples"; an unauthenticated/wrong-tenant call must
    /// not reach this method (the API layer rejects first).
    async fn query_range(
        &self,
        organization_id: Uuid,
        query: &RangeQuery,
    ) -> Result<Vec<Sample>, MetricsError>;
}

/// Skeleton in-memory store. Keeps every sample for every tenant
/// in a single mutex-protected map. Designed for the smoke test;
/// a multi-day production trace would blow it up — that's why the
/// trait above gates the eventual swap.
#[derive(Debug, Default)]
pub struct InMemoryMetricsStore {
    inner: Mutex<HashMap<Uuid, Vec<Sample>>>,
}

impl InMemoryMetricsStore {
    /// Construct an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl MetricsStore for InMemoryMetricsStore {
    async fn ingest(
        &self,
        organization_id: Uuid,
        samples: Vec<Sample>,
    ) -> Result<usize, MetricsError> {
        let n = samples.len();
        let mut guard = self.inner.lock().expect("metrics store mutex poisoned");
        guard.entry(organization_id).or_default().extend(samples);
        Ok(n)
    }

    async fn query_range(
        &self,
        organization_id: Uuid,
        query: &RangeQuery,
    ) -> Result<Vec<Sample>, MetricsError> {
        let guard = self.inner.lock().expect("metrics store mutex poisoned");
        let Some(tenant_samples) = guard.get(&organization_id) else {
            return Ok(Vec::new());
        };
        let matched: Vec<Sample> = tenant_samples
            .iter()
            .filter(|s| sample_matches(s, query))
            .cloned()
            .collect();
        Ok(matched)
    }
}

fn sample_matches(sample: &Sample, query: &RangeQuery) -> bool {
    if sample.timestamp_ms < query.start_ms || sample.timestamp_ms > query.end_ms {
        return false;
    }
    let metric_label = sample
        .labels
        .iter()
        .find(|l| l.name == "__name__")
        .map(|l| l.value.as_str());
    if metric_label != Some(query.metric.as_str()) {
        return false;
    }
    for matcher in &query.label_eq {
        if !sample
            .labels
            .iter()
            .any(|l| l.name == matcher.name && l.value == matcher.value)
        {
            return false;
        }
    }
    true
}

// ── VictoriaMetrics store ────────────────────────────────────────────────────

/// Serde shapes for the Prometheus-compatible `query_range` JSON response.
mod prom_resp {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct Envelope {
        pub status: String,
        pub data: Option<Data>,
        #[serde(rename = "errorType")]
        pub error_type: Option<String>,
        pub error: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct Data {
        pub result: Vec<Series>,
    }

    #[derive(Deserialize)]
    pub struct Series {
        pub metric: std::collections::HashMap<String, String>,
        pub values: Vec<(f64, String)>,
    }
}

/// Escapes a label value for the Prometheus text exposition format.
fn prom_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Live [`MetricsStore`] backed by a `VictoriaMetrics` single-node instance.
///
/// # Tenant isolation
///
/// `VictoriaMetrics` single-node has no built-in per-tenant auth.
/// Isolation is enforced by this store:
///
/// - **Ingest**: every sample gets an `organization_id` label injected
///   server-side. The caller-supplied labels are used as-is; a client that
///   tries to forge a different `organization_id` is overridden — the store
///   always re-stamps from the authenticated actor value passed by
///   [`handle_write`].
/// - **Query**: every `PromQL` query includes a mandatory
///   `organization_id="<uuid>"` label matcher. There is no API path that
///   issues a query without the matcher.
///
/// # Wire format
///
/// Ingest uses VM's `/api/v1/import/prometheus` endpoint (`Prometheus`
/// text exposition format — no protobuf or snappy dependency). Queries
/// use VM's `/api/v1/query_range` endpoint (standard `Prometheus` JSON
/// response).
pub struct VictoriaMetricsStore {
    client: reqwest::Client,
    /// Base URL without a trailing slash, e.g. `http://victoria:8428`.
    base_url: String,
}

impl VictoriaMetricsStore {
    /// Construct a new store.  `base_url` must be a valid HTTP/HTTPS
    /// URL; a trailing slash is stripped.
    ///
    /// # Errors
    /// Returns an error if `base_url` is not a valid URL.
    /// # Panics
    /// Panics if the underlying `reqwest` client cannot be built (should
    /// never happen with the default configuration).
    pub fn new(base_url: impl Into<String>) -> anyhow::Result<Self> {
        let raw = base_url.into();
        // Validate eagerly so misconfiguration is caught at startup.
        reqwest::Url::parse(&raw).map_err(|e| anyhow::anyhow!("victoria metrics URL: {e}"))?;
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("reqwest client build is infallible"),
            base_url: raw.trim_end_matches('/').to_owned(),
        })
    }

    /// Render samples as Prometheus text exposition lines, injecting
    /// `organization_id` as a label on every series.
    fn to_prom_text(organization_id: Uuid, samples: &[Sample]) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for s in samples {
            let name = s
                .labels
                .iter()
                .find(|l| l.name == "__name__")
                .map_or("unknown", |l| l.value.as_str());

            // Build label set: all labels except __name__ + injected org.
            let mut parts: Vec<String> = s
                .labels
                .iter()
                .filter(|l| l.name != "__name__" && l.name != "organization_id")
                .map(|l| format!("{}=\"{}\"", l.name, prom_escape(&l.value)))
                .collect();
            parts.push(format!("organization_id=\"{organization_id}\""));

            let _ = writeln!(
                out,
                "{name}{{{}}}\t{} {}",
                parts.join(","),
                s.value,
                s.timestamp_ms
            );
        }
        out
    }

    /// Build a `PromQL` instant-vector selector that matches the given
    /// `organization_id` plus any extra label equalities from `query`.
    fn promql_selector(organization_id: Uuid, query: &RangeQuery) -> String {
        let mut matchers = vec![
            format!("__name__=\"{}\"", prom_escape(&query.metric)),
            format!("organization_id=\"{}\"", organization_id),
        ];
        for l in &query.label_eq {
            matchers.push(format!("{}=\"{}\"", l.name, prom_escape(&l.value)));
        }
        format!("{{{}}}", matchers.join(","))
    }
}

#[async_trait]
impl MetricsStore for VictoriaMetricsStore {
    async fn ingest(
        &self,
        organization_id: Uuid,
        samples: Vec<Sample>,
    ) -> Result<usize, MetricsError> {
        let n = samples.len();
        if n == 0 {
            return Ok(0);
        }
        let body = Self::to_prom_text(organization_id, &samples);
        let url = format!("{}/api/v1/import/prometheus", self.base_url);
        let resp = self
            .client
            .post(&url)
            .header("Content-Type", "text/plain")
            .body(body)
            .send()
            .await
            .map_err(|e| MetricsError::Backend(e.into()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(MetricsError::Backend(anyhow::anyhow!(
                "victoria metrics ingest: {status} — {body}"
            )));
        }
        tracing::debug!(org = %organization_id, n, "metrics ingested to victoria metrics");
        Ok(n)
    }

    async fn query_range(
        &self,
        organization_id: Uuid,
        query: &RangeQuery,
    ) -> Result<Vec<Sample>, MetricsError> {
        let selector = Self::promql_selector(organization_id, query);
        // Derive a sensible step: aim for ~360 data points over the window,
        // with a floor of 15 s so very short windows don't spam the TSDB.
        let window_secs = (query.end_ms - query.start_ms).max(0) / 1000;
        let step_secs = (window_secs / 360).max(15);
        let step = format!("{step_secs}s");

        let url = format!("{}/api/v1/query_range", self.base_url);
        let resp = self
            .client
            .get(&url)
            .query(&[
                ("query", selector.as_str()),
                (
                    "start",
                    &format!(
                        "{}.{:03}",
                        query.start_ms / 1000,
                        query.start_ms.abs() % 1000
                    ),
                ),
                (
                    "end",
                    &format!("{}.{:03}", query.end_ms / 1000, query.end_ms.abs() % 1000),
                ),
                ("step", step.as_str()),
            ])
            .send()
            .await
            .map_err(|e| MetricsError::Backend(e.into()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(MetricsError::Backend(anyhow::anyhow!(
                "victoria metrics query: {status} — {body}"
            )));
        }

        let envelope: prom_resp::Envelope = resp
            .json()
            .await
            .map_err(|e| MetricsError::Backend(e.into()))?;

        if envelope.status != "success" {
            return Err(MetricsError::Backend(anyhow::anyhow!(
                "victoria metrics query error: {} — {}",
                envelope.error_type.as_deref().unwrap_or("unknown"),
                envelope.error.as_deref().unwrap_or(""),
            )));
        }

        let data = envelope
            .data
            .unwrap_or_else(|| prom_resp::Data { result: Vec::new() });

        let mut samples = Vec::new();
        for series in data.result {
            // Reconstruct label set: exclude the injected organization_id
            // (callers know the tenant; echoing it in every label is noise).
            let labels: Vec<Label> = series
                .metric
                .iter()
                .filter(|(k, _)| *k != "organization_id")
                .map(|(k, v)| Label {
                    name: k.clone(),
                    value: v.clone(),
                })
                .collect();

            for (ts_f, val_str) in series.values {
                // VM timestamps are Unix seconds as f64 (e.g. 1620000000.0).
                // Multiply by 1000 and round before truncating; the sub-second
                // precision is at most millisecond so no significant loss occurs
                // for timestamps in the reasonable range (year 1970–2262).
                #[allow(clippy::cast_possible_truncation)]
                let timestamp_ms = (ts_f * 1000.0).round() as i64;
                let value: f64 = val_str.parse().map_err(|e| {
                    MetricsError::Backend(anyhow::anyhow!("parse sample value: {e}"))
                })?;
                samples.push(Sample {
                    labels: labels.clone(),
                    timestamp_ms,
                    value,
                });
            }
        }
        Ok(samples)
    }
}

// ── Public dispatch helpers ───────────────────────────────────────────────────

/// Validate + dispatch a [`WriteRequest`]. The tenant guard is the
/// proxy's headline isolation control: even if the body claims a
/// tenant the actor has read access to (e.g. via a leaked Bearer
/// token), the body's claim is meaningless; we *only* trust the
/// authenticated tenant.
///
/// # Errors
/// - [`MetricsError::TenantMismatch`] when `actor_organization_id`
///   does not equal `request.organization_id`.
/// - [`MetricsError::Backend`] propagated from [`MetricsStore`].
pub async fn handle_write(
    store: &Arc<dyn MetricsStore>,
    actor_organization_id: Uuid,
    request: WriteRequest,
) -> Result<usize, MetricsError> {
    if actor_organization_id != request.organization_id {
        return Err(MetricsError::TenantMismatch {
            actor: actor_organization_id,
            body: request.organization_id,
        });
    }
    store.ingest(actor_organization_id, request.samples).await
}

/// Run a query scoped to the authenticated actor's tenant. The API
/// layer constructs `actor_organization_id` from the session — never
/// from the URL or body — so a wrong-tenant query is structurally
/// impossible.
///
/// # Errors
/// Propagates [`MetricsError`] from the store impl.
pub async fn handle_query(
    store: &Arc<dyn MetricsStore>,
    actor_organization_id: Uuid,
    query: &RangeQuery,
) -> Result<Vec<Sample>, MetricsError> {
    store.query_range(actor_organization_id, query).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str, ts: i64, value: f64) -> Sample {
        Sample {
            labels: vec![Label {
                name: "__name__".into(),
                value: name.into(),
            }],
            timestamp_ms: ts,
            value,
        }
    }

    fn range_for(metric: &str, start: i64, end: i64) -> RangeQuery {
        RangeQuery {
            metric: metric.into(),
            label_eq: vec![],
            start_ms: start,
            end_ms: end,
        }
    }

    #[tokio::test]
    async fn round_trip_returns_only_the_writers_samples() {
        let store: Arc<dyn MetricsStore> = Arc::new(InMemoryMetricsStore::new());
        let tenant_a = Uuid::now_v7();
        let tenant_b = Uuid::now_v7();

        handle_write(
            &store,
            tenant_a,
            WriteRequest {
                organization_id: tenant_a,
                samples: vec![sample("api_requests_total", 1000, 42.0)],
            },
        )
        .await
        .expect("tenant A ingest");

        let result_a = handle_query(&store, tenant_a, &range_for("api_requests_total", 0, 2000))
            .await
            .expect("tenant A query");
        assert_eq!(result_a.len(), 1);
        assert!((result_a[0].value - 42.0).abs() < f64::EPSILON);

        // The headline test: tenant B sees nothing of A's data.
        let result_b = handle_query(&store, tenant_b, &range_for("api_requests_total", 0, 2000))
            .await
            .expect("tenant B query");
        assert!(result_b.is_empty(), "tenant B must not see tenant A's data");
    }

    #[tokio::test]
    async fn tenant_mismatch_in_write_body_is_rejected() {
        // Defence-in-depth: even if a misbehaving (or compromised)
        // agent puts a different org id in the body, the actor's
        // session tenant is the only one that gets stored.
        let store: Arc<dyn MetricsStore> = Arc::new(InMemoryMetricsStore::new());
        let actor = Uuid::now_v7();
        let claimed = Uuid::now_v7();

        let err = handle_write(
            &store,
            actor,
            WriteRequest {
                organization_id: claimed,
                samples: vec![sample("api_requests_total", 1000, 1.0)],
            },
        )
        .await
        .expect_err("must reject tenant mismatch");
        match err {
            MetricsError::TenantMismatch { actor: a, body: b } => {
                assert_eq!(a, actor);
                assert_eq!(b, claimed);
            }
            MetricsError::Backend(e) => panic!("unexpected backend error: {e}"),
        }
    }

    #[tokio::test]
    async fn time_range_filters_out_pre_and_post_window_samples() {
        let store: Arc<dyn MetricsStore> = Arc::new(InMemoryMetricsStore::new());
        let tenant = Uuid::now_v7();
        handle_write(
            &store,
            tenant,
            WriteRequest {
                organization_id: tenant,
                samples: vec![
                    sample("cpu_usage", 100, 0.1),
                    sample("cpu_usage", 500, 0.5),
                    sample("cpu_usage", 1000, 0.9),
                ],
            },
        )
        .await
        .unwrap();

        let result = handle_query(&store, tenant, &range_for("cpu_usage", 200, 800))
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].timestamp_ms, 500);
    }

    #[tokio::test]
    async fn label_matchers_narrow_the_result() {
        let store: Arc<dyn MetricsStore> = Arc::new(InMemoryMetricsStore::new());
        let tenant = Uuid::now_v7();
        let mut s1 = sample("api_requests_total", 1000, 1.0);
        s1.labels.push(Label {
            name: "method".into(),
            value: "GET".into(),
        });
        let mut s2 = sample("api_requests_total", 1500, 1.0);
        s2.labels.push(Label {
            name: "method".into(),
            value: "POST".into(),
        });
        handle_write(
            &store,
            tenant,
            WriteRequest {
                organization_id: tenant,
                samples: vec![s1, s2],
            },
        )
        .await
        .unwrap();

        let mut q = range_for("api_requests_total", 0, 2000);
        q.label_eq.push(Label {
            name: "method".into(),
            value: "POST".into(),
        });
        let result = handle_query(&store, tenant, &q).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].timestamp_ms, 1500);
    }
}
