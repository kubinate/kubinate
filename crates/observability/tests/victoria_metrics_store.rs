//! Integration tests for [`VictoriaMetricsStore`] using a wiremock server.
//!
//! Each test stands up a local HTTP mock, drives the store against it, and
//! asserts on both the outbound request shape and the inbound response
//! mapping. No real VictoriaMetrics instance is required.

use kubinate_observability::metrics::{Label, RangeQuery, Sample, VictoriaMetricsStore, MetricsStore};
use uuid::Uuid;
use wiremock::{
    matchers::{method, path, query_param_contains},
    Mock, MockServer, ResponseTemplate,
};

// ── helpers ──────────────────────────────────────────────────────────────────

fn named_sample(name: &str, ts: i64, value: f64) -> Sample {
    Sample {
        labels: vec![Label {
            name: "__name__".into(),
            value: name.into(),
        }],
        timestamp_ms: ts,
        value,
    }
}

fn prom_matrix_response(series: &[(&str, &[(f64, &str)])]) -> serde_json::Value {
    let result: Vec<serde_json::Value> = series
        .iter()
        .map(|(metric_name, values)| {
            serde_json::json!({
                "metric": { "__name__": metric_name },
                "values": values.iter().map(|(ts, v)| serde_json::json!([ts, v])).collect::<Vec<_>>()
            })
        })
        .collect();

    serde_json::json!({
        "status": "success",
        "data": {
            "resultType": "matrix",
            "result": result
        }
    })
}

// ── ingest tests ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn ingest_posts_to_prometheus_import_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/import/prometheus"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    let org = Uuid::now_v7();
    let n = store
        .ingest(org, vec![named_sample("cpu_usage", 1_000, 0.5)])
        .await
        .expect("ingest");

    assert_eq!(n, 1);
}

#[tokio::test]
async fn ingest_includes_organization_id_label_in_body() {
    let server = MockServer::start().await;
    let org = Uuid::now_v7();
    let org_str = org.to_string();

    Mock::given(method("POST"))
        .and(path("/api/v1/import/prometheus"))
        .and(wiremock::matchers::body_string_contains(org_str))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    store
        .ingest(org, vec![named_sample("http_requests", 1_000, 10.0)])
        .await
        .expect("ingest");
}

#[tokio::test]
async fn ingest_empty_slice_skips_http_call() {
    let server = MockServer::start().await;
    // No mock registered — any HTTP call would panic the server.

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    let n = store.ingest(Uuid::now_v7(), vec![]).await.expect("ingest empty");
    assert_eq!(n, 0);
}

#[tokio::test]
async fn ingest_returns_backend_error_on_non_2xx() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/import/prometheus"))
        .respond_with(ResponseTemplate::new(503).set_body_string("overloaded"))
        .mount(&server)
        .await;

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    let err = store
        .ingest(Uuid::now_v7(), vec![named_sample("m", 1, 1.0)])
        .await
        .expect_err("should fail");

    let msg = err.to_string();
    assert!(msg.contains("503"), "error should mention status: {msg}");
}

// ── query tests ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn query_injects_organization_id_into_promql() {
    let server = MockServer::start().await;
    let org = Uuid::now_v7();
    let org_str = org.to_string();

    Mock::given(method("GET"))
        .and(path("/api/v1/query_range"))
        .and(query_param_contains("query", org_str.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(prom_matrix_response(&[])),
        )
        .expect(1)
        .mount(&server)
        .await;

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    store
        .query_range(
            org,
            &RangeQuery {
                metric: "cpu_usage".into(),
                label_eq: vec![],
                start_ms: 0,
                end_ms: 60_000,
            },
        )
        .await
        .expect("query");
}

#[tokio::test]
async fn query_deserialises_matrix_response_into_samples() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/query_range"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(prom_matrix_response(&[(
                "cpu_usage",
                &[(1_620_000_000.0_f64, "0.75"), (1_620_000_015.0, "0.80")],
            )])),
        )
        .mount(&server)
        .await;

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    let samples = store
        .query_range(
            Uuid::now_v7(),
            &RangeQuery {
                metric: "cpu_usage".into(),
                label_eq: vec![],
                start_ms: 1_620_000_000_000,
                end_ms: 1_620_000_060_000,
            },
        )
        .await
        .expect("query");

    assert_eq!(samples.len(), 2);
    assert!((samples[0].value - 0.75).abs() < 1e-9);
    assert!((samples[1].value - 0.80).abs() < 1e-9);
    // Timestamp is converted from Unix-seconds float to milliseconds.
    assert_eq!(samples[0].timestamp_ms, 1_620_000_000_000);
}

#[tokio::test]
async fn query_strips_organization_id_from_returned_labels() {
    let server = MockServer::start().await;
    let org = Uuid::now_v7();

    let resp = serde_json::json!({
        "status": "success",
        "data": {
            "resultType": "matrix",
            "result": [{
                "metric": {
                    "__name__": "http_requests_total",
                    "organization_id": org.to_string(),
                    "method": "GET"
                },
                "values": [[1_620_000_000.0_f64, "42"]]
            }]
        }
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/query_range"))
        .respond_with(ResponseTemplate::new(200).set_body_json(resp))
        .mount(&server)
        .await;

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    let samples = store
        .query_range(
            org,
            &RangeQuery {
                metric: "http_requests_total".into(),
                label_eq: vec![],
                start_ms: 0,
                end_ms: 2_000_000_000_000,
            },
        )
        .await
        .expect("query");

    assert_eq!(samples.len(), 1);
    // organization_id must be stripped — it's ambient context, not a label.
    assert!(
        !samples[0].labels.iter().any(|l| l.name == "organization_id"),
        "organization_id should not appear in returned labels"
    );
    assert!(samples[0].labels.iter().any(|l| l.name == "method" && l.value == "GET"));
}

#[tokio::test]
async fn query_returns_backend_error_on_non_2xx() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/query_range"))
        .respond_with(ResponseTemplate::new(429).set_body_string("too many requests"))
        .mount(&server)
        .await;

    let store = VictoriaMetricsStore::new(server.uri()).expect("store init");
    let err = store
        .query_range(
            Uuid::now_v7(),
            &RangeQuery {
                metric: "cpu".into(),
                label_eq: vec![],
                start_ms: 0,
                end_ms: 1_000,
            },
        )
        .await
        .expect_err("should fail");

    assert!(err.to_string().contains("429"), "error should mention status");
}
