//! `/v1/clusters` — create, read (ticket 02).
//!
//! The POST handler supports `Idempotency-Key` for safe retries: the
//! (organization, key) pair is stored alongside a SHA-256 hash of the
//! canonical request body and the response. A retry with the same key
//! and body replays the cached response; a retry with a mismatched
//! body returns `409 Conflict`.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use futures::stream::{self, Stream};
use kubinate_cluster::{
    model::{Cluster, NewCluster},
    service::{ALLOWED_REGIONS, ALLOWED_SERVER_TYPES},
    status::{error_category, ErrorCategory},
};
use kubinate_integrations::hetzner::Client as HetznerClient;
use kubinate_platform::{audit, error::PlatformError};
use kubinate_workflows::events::ClusterEvent;
use kubinate_workflows::workflows::ProvisionClusterInput;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::{convert::Infallible, time::Duration as StdDuration};
use time::{Duration, OffsetDateTime};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use uuid::Uuid;

use crate::{
    actor::{Actor, OwnerActor},
    audit_ctx,
    problem::ApiError,
    AppState,
};

const IDEMPOTENCY_HEADER: &str = "idempotency-key";
const IDEMPOTENCY_TTL: Duration = Duration::hours(24);

/// Mount cluster routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(create).get(list))
        .route("/:id", get(read).delete(destroy))
        .route("/:id/kubeconfig", get(download_kubeconfig))
        .route("/:id/addons", post(install_addon).get(list_addons))
        .route("/:id/workers", post(scale_workers))
        .route("/:id/events", get(stream_events))
}

#[derive(Deserialize, Serialize)]
struct CreateRequest {
    name: String,
    region: String,
    server_type: String,
    #[serde(default = "default_cp")]
    control_plane_count: i16,
    worker_count: i16,
    credential_id: Uuid,
}

fn default_cp() -> i16 {
    1
}

#[derive(Serialize)]
struct ClusterView {
    id: Uuid,
    name: String,
    region: String,
    server_type: String,
    control_plane_count: i16,
    worker_count: i16,
    status: kubinate_cluster::model::ClusterStatus,
    /// Provisioning step, if a workflow is active. Comes from the
    /// `provisioning_workflows` shadow table.
    current_step: Option<String>,
    /// When the workflow started (workflow row's `started_at`); falls
    /// back to the cluster's `created_at` for clusters that haven't
    /// triggered a workflow yet.
    started_at: OffsetDateTime,
    /// Stable category the UI maps to a friendly message. `None`
    /// unless `status == failed`.
    error_category: Option<&'static str>,
    /// Whether terminal — UI uses this to stop polling.
    terminal: bool,
    /// Whether a kubeconfig is ready to download.
    kubeconfig_available: bool,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl ClusterView {
    fn from_parts(
        c: Cluster,
        current_step: Option<String>,
        workflow_started_at: Option<OffsetDateTime>,
    ) -> Self {
        let category =
            error_category(c.status, c.status_reason.as_deref()).map(ErrorCategory::as_str);
        let terminal = matches!(
            c.status,
            kubinate_cluster::model::ClusterStatus::Ready
                | kubinate_cluster::model::ClusterStatus::Failed
                | kubinate_cluster::model::ClusterStatus::Destroyed,
        );
        Self {
            id: c.id,
            name: c.name,
            region: c.region,
            server_type: c.server_type,
            control_plane_count: c.control_plane_count,
            worker_count: c.worker_count,
            status: c.status,
            current_step,
            started_at: workflow_started_at.unwrap_or(c.created_at),
            error_category: category,
            terminal,
            kubeconfig_available: matches!(c.status, kubinate_cluster::model::ClusterStatus::Ready),
            created_at: c.created_at,
            updated_at: c.updated_at,
        }
    }
}

impl From<Cluster> for ClusterView {
    fn from(c: Cluster) -> Self {
        Self::from_parts(c, None, None)
    }
}

#[derive(Serialize)]
#[allow(dead_code)]
struct CatalogResponse<'a> {
    regions: &'a [&'a str],
    server_types: &'a [&'a str],
}

async fn create(
    State(state): State<AppState>,
    owner: OwnerActor,
    headers: HeaderMap,
    body: String,
) -> Result<impl IntoResponse, ApiError> {
    let actor = owner.inner;
    let req: CreateRequest = serde_json::from_str(&body)
        .map_err(|e| ApiError::from(PlatformError::Invalid(format!("invalid JSON body: {e}"))))?;

    let body_hash = Sha256::digest(body.as_bytes()).to_vec();
    let idempotency_key = headers
        .get(IDEMPOTENCY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // Replay path: we've seen this (org, key) before. Return the cached
    // response if the body hash matches, or 409 if the client is
    // reusing a key for a different payload.
    if let Some(ref key) = idempotency_key {
        if let Some(cached) =
            lookup_idempotency(&state, actor.organization_id, key, &body_hash).await?
        {
            return Ok((
                StatusCode::from_u16(u16::try_from(cached.response_status).unwrap_or(200))
                    .unwrap_or(StatusCode::OK),
                Json(cached.response_body),
            )
                .into_response());
        }
    }

    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    let cluster = state
        .cluster_service
        .create(
            actor.organization_id,
            NewCluster {
                name: req.name.clone(),
                region: req.region.clone(),
                server_type: req.server_type.clone(),
                control_plane_count: req.control_plane_count,
                worker_count: req.worker_count,
                credential_id: req.credential_id,
            },
            &audit,
        )
        .await?;

    // Resolve the credential, build a Hetzner client bound to its
    // SecretRef, and kick off the provision workflow on the runtime.
    // The handler returns 202 immediately; the dashboard's status
    // poll (ticket 06) takes over from here.
    let credential = state
        .hetzner_service
        .get(actor.organization_id, req.credential_id)
        .await?;
    let hetzner: Arc<dyn kubinate_integrations::hetzner::HetznerProvider> = Arc::new(
        HetznerClient::new(state.secret_store.clone(), credential.secret_ref),
    );
    state.runner.spawn_provision(
        actor.organization_id,
        hetzner,
        ProvisionClusterInput {
            cluster_id: cluster.id,
            cluster_name: cluster.name.clone(),
            location: cluster.region.clone(),
            server_type: cluster.server_type.clone(),
            worker_count: usize::try_from(cluster.worker_count.max(0)).unwrap_or(0),
            ssh_key: state.cluster_settings.ssh_key.clone(),
            user_data: state.cluster_settings.user_data.clone(),
            k3s_version: state.cluster_settings.k3s_version.clone(),
        },
    );

    let view = ClusterView::from(cluster.clone());
    let response_body = serde_json::to_value(&view).expect("serialize cluster");

    if let Some(ref key) = idempotency_key {
        store_idempotency(
            &state,
            actor.organization_id,
            key,
            &body_hash,
            202,
            &response_body,
            Some(cluster.id),
        )
        .await?;
    }

    Ok((StatusCode::ACCEPTED, Json(response_body)).into_response())
}

async fn read(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> Result<Json<ClusterView>, ApiError> {
    let cluster = state.cluster_service.get(actor.organization_id, id).await?;

    // Best-effort look up the workflow shadow row. Missing workflow row
    // is fine — it just means the runner hasn't started one yet.
    let mut tx = state.db.begin().await.map_err(PlatformError::from)?;
    sqlx::query(&format!(
        "SET LOCAL app.current_tenant_id = '{}'",
        actor.organization_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(PlatformError::from)?;
    let workflow: Option<(String, OffsetDateTime)> = sqlx::query_as(
        r"
        SELECT current_step::text, started_at
        FROM provisioning_workflows
        WHERE cluster_id = $1
        ORDER BY started_at DESC
        LIMIT 1
        ",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(PlatformError::from)?;
    tx.commit().await.map_err(PlatformError::from)?;

    let (step, started_at) = match workflow {
        Some((s, ts)) => (Some(s), Some(ts)),
        None => (None, None),
    };
    Ok(Json(ClusterView::from_parts(cluster, step, started_at)))
}

async fn list(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<Json<Vec<ClusterView>>, ApiError> {
    let rows = state.cluster_service.list(actor.organization_id).await?;
    Ok(Json(rows.into_iter().map(ClusterView::from).collect()))
}

/// `DELETE /v1/clusters/:id` — initiate destroy. Returns 202; the
/// runner runs the workflow asynchronously.
///
/// Sprint 4 ticket 05 — first route migrated to [`OwnerActor`]. The
/// extractor enforces two gates **before** the handler runs:
///
/// 1. **MFA gate.** A partial-MFA session (`mfa_satisfied = false`)
///    is rejected with HTTP 401 and Problem Details
///    `code = mfa_required`; the SPA's `_problem.ts` discriminator
///    catches it and routes to the `WebAuthn` challenge page.
/// 2. **Role gate.** A live membership in the active organization
///    must be Owner or Admin. Member / Developer / Viewer roles get
///    HTTP 403.
///
/// We pick destroy as the proof-of-concept route because it has the
/// largest blast radius of any existing handler — once submitted,
/// the workflow cannot be undone, and the Hetzner servers do incur
/// cost while running. A bypassed MFA gate on this route is the
/// concrete impact the gate is designed to prevent.
async fn destroy(
    State(state): State<AppState>,
    owner: OwnerActor,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let actor = owner.inner;
    let cluster = state.cluster_service.get(actor.organization_id, id).await?;
    let credential = state
        .hetzner_service
        .get(actor.organization_id, cluster.credential_id)
        .await?;
    let hetzner: Arc<dyn kubinate_integrations::hetzner::HetznerProvider> = Arc::new(
        HetznerClient::new(state.secret_store.clone(), credential.secret_ref),
    );

    state
        .runner
        .spawn_destroy(actor.organization_id, hetzner, id);

    Ok(StatusCode::ACCEPTED)
}

/// `GET /v1/clusters/:id/kubeconfig` — serves the plaintext kubeconfig
/// as a downloadable attachment. Tier-1 sensitive (kubeconfig grants
/// cluster-admin), so:
///
/// 1. The handler is rate-limited at 5 requests/min/user (ticket 04 `DoD`).
/// 2. Every successful retrieval appends a `kubeconfig.retrieved`
///    audit row via `audit_log_append_explicit` — without this the
///    audit chain has no record of reads, since SELECT does not fire
///    the per-table mutation trigger.
/// 3. The plaintext is materialised in a `SecretString` for the
///    smallest possible window before being written into the response
///    body and dropped (`secrecy` zeroes on drop).
async fn download_kubeconfig(
    State(state): State<AppState>,
    actor: Actor,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Response, ApiError> {
    if !state.kubeconfig_rate_limit.check(actor.user_id) {
        return Err(ApiError::from(PlatformError::Forbidden(
            "kubeconfig downloads are rate-limited to 5 per minute".into(),
        )));
    }

    let kubeconfig = state
        .cluster_service
        .fetch_kubeconfig(actor.organization_id, id)
        .await?;

    record_kubeconfig_audit(&state, &actor, &headers, id).await?;

    let filename = format!("kubinate-{id}.yaml");
    let body = kubeconfig.expose_secret().to_string();

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/yaml")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        // kubeconfigs are tier-1 sensitive — never let an intermediary
        // (including Cloudflare) cache them.
        .header(header::CACHE_CONTROL, "no-store, max-age=0")
        .body(Body::from(body))
        .map_err(|e| ApiError::from(PlatformError::Internal(anyhow::anyhow!(e))))
}

async fn record_kubeconfig_audit(
    state: &AppState,
    actor: &Actor,
    headers: &HeaderMap,
    cluster_id: Uuid,
) -> Result<(), PlatformError> {
    let mut tx = state.db.begin().await?;
    sqlx::query(&format!(
        "SET LOCAL app.current_tenant_id = '{}'",
        actor.organization_id
    ))
    .execute(&mut *tx)
    .await?;

    let ctx = audit::AuditContext {
        actor_user_id: Some(actor.user_id).filter(|u| !u.is_nil()),
        request_id: headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
        ip: None,
        user_agent: headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned),
    };
    ctx.apply(&mut tx).await?;

    audit::append_explicit(
        &mut tx,
        actor.organization_id,
        "kubeconfig.retrieved",
        "cluster",
        Some(&cluster_id.to_string()),
        "allowed",
        serde_json::json!({}),
    )
    .await?;

    tx.commit().await?;
    Ok(())
}

struct CachedResponse {
    response_status: i16,
    response_body: serde_json::Value,
}

async fn lookup_idempotency(
    state: &AppState,
    organization_id: Uuid,
    key: &str,
    body_hash: &[u8],
) -> Result<Option<CachedResponse>, PlatformError> {
    let mut tx = state.db.begin().await?;
    sqlx::query(&format!(
        "SET LOCAL app.current_tenant_id = '{organization_id}'",
    ))
    .execute(&mut *tx)
    .await?;

    let row: Option<(Vec<u8>, i16, serde_json::Value, OffsetDateTime)> = sqlx::query_as(
        r"
        SELECT request_hash, response_status, response_body, expires_at
        FROM idempotency_keys
        WHERE organization_id = $1 AND key = $2
        ",
    )
    .bind(organization_id)
    .bind(key)
    .fetch_optional(&mut *tx)
    .await?;
    tx.commit().await?;

    let Some((stored_hash, status, body, expires_at)) = row else {
        return Ok(None);
    };

    if expires_at < OffsetDateTime::now_utc() {
        // Row expired; ignore it and let the caller create a fresh one.
        return Ok(None);
    }

    if stored_hash != body_hash {
        return Err(PlatformError::Conflict(
            "Idempotency-Key reused with a different request body".into(),
        ));
    }

    Ok(Some(CachedResponse {
        response_status: status,
        response_body: body,
    }))
}

async fn store_idempotency(
    state: &AppState,
    organization_id: Uuid,
    key: &str,
    body_hash: &[u8],
    status: u16,
    body: &serde_json::Value,
    resource_id: Option<Uuid>,
) -> Result<(), PlatformError> {
    let mut tx = state.db.begin().await?;
    sqlx::query(&format!(
        "SET LOCAL app.current_tenant_id = '{organization_id}'",
    ))
    .execute(&mut *tx)
    .await?;

    let now = OffsetDateTime::now_utc();
    sqlx::query(
        r"
        INSERT INTO idempotency_keys
            (organization_id, key, request_hash, response_status,
             response_body, resource_id, created_at, expires_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (organization_id, key) DO NOTHING
        ",
    )
    .bind(organization_id)
    .bind(key)
    .bind(body_hash)
    .bind(i16::try_from(status).unwrap_or(0))
    .bind(body)
    .bind(resource_id)
    .bind(now)
    .bind(now + IDEMPOTENCY_TTL)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// `GET /v1/clusters/:id/events` — Server-Sent Events stream of
/// cluster lifecycle transitions (Sprint 3 ticket 10).
///
/// Tenant scoping is enforced by `cluster_service.get` first — a
/// request for a cluster the actor cannot see returns 404 before the
/// stream opens. After that the handler subscribes to the per-cluster
/// broadcast channel and surfaces:
///
/// - `event: step` for every workflow step transition (mirrors the
///   shadow-table writes the runner already does).
/// - `event: terminal` once when the cluster reaches `ready` /
///   `failed` / `destroyed`. The handler then closes the stream so
///   the browser doesn't sit on a dangling connection.
///
/// Keep-alive comments are sent every 15s to defeat intermediary
/// buffering (Cloudflare in particular holds idle SSE streams open
/// only with a regular ping).
async fn stream_events(
    State(state): State<AppState>,
    actor: Actor,
    Path(cluster_id): Path<Uuid>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    // Authorisation: this also catches "no such cluster" so we don't
    // open a stream for a cluster the actor can't see.
    state
        .cluster_service
        .get(actor.organization_id, cluster_id)
        .await?;

    let receiver = state.event_hub.subscribe(cluster_id);

    // BroadcastStream surfaces `Lagged` errors when a slow consumer
    // falls behind the channel capacity. We translate those into a
    // sentinel `event: lagged` rather than killing the connection,
    // and let the browser refetch its initial state.
    let raw = BroadcastStream::new(receiver).filter_map(|item| match item {
        Ok(event) => Some(event),
        Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => {
            Some(ClusterEvent::Step {
                step: "__lagged__".to_string(),
            })
        }
    });

    // Stop the stream after the terminal event so EventSource on the
    // client side closes cleanly instead of seeing a half-open socket.
    let mut terminal_seen = false;
    let limited = raw.take_while(move |event| {
        if terminal_seen {
            return false;
        }
        if matches!(event, ClusterEvent::Terminal { .. }) {
            terminal_seen = true;
        }
        true
    });

    let mapped = limited.map(|event| {
        let (kind, data) = match &event {
            ClusterEvent::Step { .. } => ("step", &event),
            ClusterEvent::Terminal { .. } => ("terminal", &event),
        };
        let body = serde_json::to_string(data)
            .unwrap_or_else(|_| "{\"type\":\"step\",\"step\":\"__error__\"}".to_string());
        Ok(Event::default().event(kind).data(body))
    });

    // Prepend a synthetic `connected` event so the browser knows the
    // stream is live before any real workflow event arrives.
    let prelude = stream::once(async {
        Ok::<Event, Infallible>(Event::default().event("connected").data("{}"))
    });

    let stream = prelude.chain(mapped);

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(StdDuration::from_secs(15))
            .text(": keep-alive"),
    ))
}

/// `GET /v1/catalog/clusters` — frontend reads this so the form's
/// dropdowns stay in sync with the API allowlist.
pub async fn catalog() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "regions": ALLOWED_REGIONS,
        "server_types": ALLOWED_SERVER_TYPES,
        "addons": kubinate_addons::catalog::slugs(),
    }))
}

#[derive(Deserialize)]
struct InstallAddonRequest {
    addon: String,
    version: String,
}

#[derive(Serialize)]
struct AddonView {
    id: Uuid,
    addon: String,
    version: String,
    helm_release: String,
    status: kubinate_addons::model::AddonStatus,
    status_reason: Option<String>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
}

impl From<kubinate_addons::model::ClusterAddon> for AddonView {
    fn from(a: kubinate_addons::model::ClusterAddon) -> Self {
        Self {
            id: a.id,
            addon: a.addon,
            version: a.version,
            helm_release: a.helm_release,
            status: a.status,
            status_reason: a.status_reason,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

/// `POST /v1/clusters/:id/addons` — request an addon install.
/// Idempotent on `(cluster, addon)` per ticket 07 AC #3.
async fn install_addon(
    State(state): State<AppState>,
    owner: OwnerActor,
    headers: HeaderMap,
    Path(cluster_id): Path<Uuid>,
    Json(req): Json<InstallAddonRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let actor = owner.inner;
    // Cluster must be Ready before we attempt an install — Helm will
    // hang otherwise.
    let cluster = state
        .cluster_service
        .get(actor.organization_id, cluster_id)
        .await?;
    if !matches!(
        cluster.status,
        kubinate_cluster::model::ClusterStatus::Ready
    ) {
        return Err(ApiError::from(PlatformError::Conflict(
            "cluster must be Ready before installing addons".into(),
        )));
    }

    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    let row = state
        .addon_service
        .request_install(
            actor.organization_id,
            cluster_id,
            kubinate_addons::model::NewAddon {
                addon: req.addon.clone(),
                version: req.version.clone(),
            },
            &audit,
        )
        .await?;

    // Materialise the kubeconfig once; the runner removes the
    // tempfile after Helm finishes.
    let kubeconfig = state
        .cluster_service
        .fetch_kubeconfig(actor.organization_id, cluster_id)
        .await?;
    let kubeconfig_path = std::env::temp_dir().join(format!("kubinate-kc-{}.yaml", Uuid::now_v7()));
    if let Err(err) = tokio::fs::write(&kubeconfig_path, kubeconfig.expose_secret()).await {
        return Err(ApiError::from(PlatformError::Internal(anyhow::anyhow!(
            "stage kubeconfig: {err}"
        ))));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            tokio::fs::set_permissions(&kubeconfig_path, std::fs::Permissions::from_mode(0o600))
                .await;
    }

    let spec = kubinate_addons::catalog::lookup(&row.addon)?;
    let params = kubinate_integrations::helm::InstallParams {
        release: row.helm_release.clone(),
        chart: spec.chart_name.to_string(),
        repo: spec.chart_repo.to_string(),
        version: row.version.clone(),
        namespace: spec.namespace.to_string(),
        values_yaml: spec.default_values_yaml.to_string(),
    };
    state
        .runner
        .spawn_install_addon(actor.organization_id, row.id, params, kubeconfig_path);

    Ok((StatusCode::ACCEPTED, Json(AddonView::from(row))))
}

#[derive(Deserialize)]
struct ScaleWorkersRequest {
    /// Signed worker delta. `+N` adds workers, `-N` removes them,
    /// `0` is a no-op.
    delta: i16,
}

const MIN_WORKER_COUNT: i16 = 1;
const MAX_WORKER_COUNT: i16 = 10;

/// `POST /v1/clusters/:id/workers` — scale a Ready cluster up or down
/// by `delta` workers. Returns 202 once the workflow has been
/// dispatched, or 200 with the current view when `delta` is zero.
async fn scale_workers(
    State(state): State<AppState>,
    owner: OwnerActor,
    headers: HeaderMap,
    Path(cluster_id): Path<Uuid>,
    Json(req): Json<ScaleWorkersRequest>,
) -> Result<Response, ApiError> {
    let actor = owner.inner;
    let cluster = state
        .cluster_service
        .get(actor.organization_id, cluster_id)
        .await?;

    if !matches!(
        cluster.status,
        kubinate_cluster::model::ClusterStatus::Ready
    ) {
        return Err(ApiError::from(PlatformError::Conflict(
            "cluster must be Ready to scale".into(),
        )));
    }

    if req.delta == 0 {
        // No-op idempotency: nothing to do, hand back the current
        // view so the client can refresh its UI.
        return Ok((StatusCode::OK, Json(ClusterView::from(cluster))).into_response());
    }

    let new_worker_count = cluster.worker_count.saturating_add(req.delta);
    if !(MIN_WORKER_COUNT..=MAX_WORKER_COUNT).contains(&new_worker_count) {
        return Err(ApiError::from(PlatformError::Invalid(format!(
            "worker_count must stay between {MIN_WORKER_COUNT} and {MAX_WORKER_COUNT} \
             (current {} + delta {} = {new_worker_count})",
            cluster.worker_count, req.delta
        ))));
    }

    let credential = state
        .hetzner_service
        .get(actor.organization_id, cluster.credential_id)
        .await?;
    let hetzner: Arc<dyn kubinate_integrations::hetzner::HetznerProvider> = Arc::new(
        HetznerClient::new(state.secret_store.clone(), credential.secret_ref),
    );

    let audit = audit_ctx::from_actor_and_headers(&actor, &headers);
    state
        .cluster_repo
        .update_worker_count(actor.organization_id, cluster_id, new_worker_count, &audit)
        .await?;

    if req.delta > 0 {
        #[allow(clippy::cast_sign_loss)]
        let count = req.delta as usize;
        state.runner.spawn_scale_out(
            actor.organization_id,
            hetzner,
            cluster_id,
            count,
            cluster.name.clone(),
            cluster.region.clone(),
            cluster.server_type.clone(),
            state.cluster_settings.ssh_key.clone(),
            state.cluster_settings.user_data.clone(),
            state.cluster_settings.k3s_version.clone(),
        );
    } else {
        #[allow(clippy::cast_sign_loss)]
        let count = (-req.delta) as usize;
        state.runner.spawn_scale_in(
            actor.organization_id,
            hetzner,
            cluster_id,
            cluster.name.clone(),
            count,
        );
    }

    let mut view = ClusterView::from(cluster);
    view.worker_count = new_worker_count;
    view.status = kubinate_cluster::model::ClusterStatus::Scaling;
    view.terminal = false;
    Ok((StatusCode::ACCEPTED, Json(view)).into_response())
}

/// `GET /v1/clusters/:id/addons` — list installed / pending addons.
async fn list_addons(
    State(state): State<AppState>,
    actor: Actor,
    Path(cluster_id): Path<Uuid>,
) -> Result<Json<Vec<AddonView>>, ApiError> {
    let rows = state
        .addon_service
        .list_for_cluster(actor.organization_id, cluster_id)
        .await?;
    Ok(Json(rows.into_iter().map(AddonView::from).collect()))
}
