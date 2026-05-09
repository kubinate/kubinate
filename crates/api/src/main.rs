//! Kubinate API binary.
//!
//! Phase-0 shape: a minimal Axum server that starts cleanly, exposes
//! a health check, pulls configuration from the environment, sets up
//! structured logging + tracing, and shuts down gracefully.
//!
//! Sprint 1 wires the first feature routes:
//! * `/v1/auth/github/*`        (ticket 07 — sign-in)
//! * `/v1/integrations/hetzner` (ticket 01 — credential storage)

#![forbid(unsafe_code)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

mod actor;
mod agent;
mod audit_ctx;
mod auth;
mod auth_passkey;
mod billing;
mod clusters;
mod integrations;
mod observability;
mod problem;
mod rate_limit;
mod team;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::Context;
use axum::{
    extract::State,
    http::{HeaderName, StatusCode},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use kubinate_cluster::{repository::PgClusterRepository, service::ClusterService};
use kubinate_identity::{
    repository::{PgHetznerCredentialRepository, PgInviteRepository, PgMembershipRepository},
    service::{HetznerCredentialService, MembershipService},
};
use kubinate_integrations::{
    github::HttpGithubClient,
    helm::HelmCliExecutor,
    kubectl::KubectlCliExecutor,
    ssh::OpensshExecutor,
    stripe::{HttpStripeClient, StripeClient},
};
use kubinate_platform::{
    config::AppConfig,
    db,
    secrets::{PgcryptoStore, SecretStore},
    telemetry,
};
use kubinate_workflows::runner::LocalRunner;
use secrecy::SecretString;
use serde::Serialize;
use tokio::signal;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

use crate::{
    auth::{AuthConfig, SharedGithubClient},
    problem::ApiError,
    rate_limit::RateLimiter,
};

#[derive(Clone)]
struct AppState {
    db: sqlx::PgPool,
    started_at: time::OffsetDateTime,
    version: &'static str,
    hetzner_service: Arc<HetznerCredentialService>,
    cluster_service: Arc<ClusterService>,
    addon_service: Arc<kubinate_addons::service::AddonService>,
    membership_service: Arc<MembershipService>,
    billing_service: Arc<kubinate_billing::service::BillingService>,
    stripe_webhook_secret: SecretString,
    cluster_repo: Arc<dyn kubinate_cluster::repository::ClusterRepository>,
    secret_store: Arc<dyn SecretStore>,
    runner: Arc<LocalRunner>,
    github: SharedGithubClient,
    auth: Arc<AuthConfig>,
    kubeconfig_rate_limit: Arc<RateLimiter>,
    cluster_settings: Arc<ClusterSettings>,
    /// Per-cluster SSE event hub. Cloned by the runner (publishes)
    /// and the `/v1/clusters/:id/events` handler (subscribes).
    event_hub: Arc<kubinate_workflows::events::ClusterEventHub>,
    /// Observability proxy backend. JSON in-memory scaffold today;
    /// VictoriaMetrics-backed impl post-spike (see
    /// `docs/decisions/sprint-3-observability-tsdb.md`).
    metrics_store: Arc<dyn kubinate_observability::metrics::MetricsStore>,
    /// Sprint 4 ticket 05 — `WebAuthn` ceremony facade. `None` when
    /// the env vars are not set; `auth_passkey` routes return 503
    /// in that case while the rest of the API works unchanged.
    ceremonies: Option<Arc<kubinate_identity::webauthn::Ceremonies>>,
    /// Per-user passkey persistence.
    passkey_repo: Arc<dyn kubinate_identity::repository::PasskeyRepository>,
    /// Per-user MFA recovery-code persistence.
    recovery_codes_repo: Arc<dyn kubinate_identity::repository::RecoveryCodeRepository>,
    /// In-memory association between an open registration ceremony
    /// id and the user-supplied nickname for the new passkey.
    /// Bounded by the ceremony's 5-minute TTL; a periodic sweep is
    /// a follow-up if the row count ever matters at scale.
    passkey_pending_nicknames: Arc<std::sync::Mutex<std::collections::HashMap<uuid::Uuid, String>>>,
}

/// Bootstrap configuration the runner needs to populate
/// `ProvisionClusterInput` for every workflow start.
#[derive(Debug, Clone)]
struct ClusterSettings {
    /// Hetzner SSH key resource (id or name) registered in every
    /// tenant's project. Sprint-1 simplification: one operator key.
    ssh_key: String,
    /// k3s version installed by the SSH activity.
    k3s_version: String,
    /// cloud-init document handed to every k3s node on first boot.
    user_data: String,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    telemetry::init("kubinate-api").context("telemetry init")?;
    kubinate_platform::metrics::init().context("metrics init")?;
    let config = AppConfig::load().context("config load")?;

    tracing::info!(env = %config.environment, "starting kubinate-api");

    // Sprint 4 ticket 07 — Postgres role split. Migrations run as
    // the bootstrap user (which retains DDL + role-management
    // privileges); the runtime pool connects as the non-superuser
    // `kubinate_app` role so RLS on tenant-scoped tables actually
    // applies (Postgres superusers bypass RLS even with `FORCE ROW
    // LEVEL SECURITY`).
    //
    // Backwards-compatible default: when `KUBINATE__DATABASE_MIGRATION_URL`
    // is unset, fall back to the runtime URL — that's the dev /
    // legacy shape where the same role does both. Production +
    // CI set the var explicitly.
    let migration_url = std::env::var("KUBINATE__DATABASE_MIGRATION_URL")
        .unwrap_or_else(|_| config.database_url.clone());
    {
        let migration_pool = db::pool(&migration_url, 4)
            .await
            .context("migration pool")?;
        db::migrate(&migration_pool).await.context("migrations")?;
        // Drop the migration pool before opening the runtime pool so
        // the bootstrap user's connection slots are returned to the
        // server. The runtime pool owns its own slots.
        migration_pool.close().await;
    }
    let pool = db::pool(&config.database_url, 20)
        .await
        .context("database pool")?;

    // Sprint 4 ticket 02 — backend switch. Default `pgcrypto` keeps
    // every existing deploy unchanged. `vault` requires the
    // KUBINATE__VAULT_ADDR + KUBINATE__VAULT_TOKEN pair and routes
    // every put/get/delete through Vault's transit engine. The
    // `wrong-backend` failure shape is loud (`Decrypt`) per the
    // parity tests — flipping mid-flight on a populated DB is not a
    // supported operation; the migration binary (Sprint 5+) does
    // the re-key.
    let secret_store: Arc<dyn SecretStore> = match std::env::var("KUBINATE__SECRETS_BACKEND")
        .as_deref()
        .unwrap_or("pgcrypto")
    {
        "pgcrypto" => {
            Arc::new(PgcryptoStore::from_env(pool.clone()).context("secret store init (pgcrypto)")?)
        }
        "vault" => {
            let transit = kubinate_platform::secrets::HttpVaultTransit::from_env().context(
                "secret store init (vault) — KUBINATE__VAULT_ADDR + KUBINATE__VAULT_TOKEN required",
            )?;
            let key_prefix = std::env::var("KUBINATE__VAULT_KEY_PREFIX")
                .unwrap_or_else(|_| "kubinate".to_string());
            tracing::info!(prefix = %key_prefix, "secret store: vault transit engine");
            Arc::new(kubinate_platform::secrets::VaultStore::new(
                pool.clone(),
                transit,
                key_prefix,
            ))
        }
        other => {
            anyhow::bail!("KUBINATE__SECRETS_BACKEND must be 'pgcrypto' or 'vault'; got '{other}'");
        }
    };
    let credential_repo = Arc::new(PgHetznerCredentialRepository::new(pool.clone()));
    let hetzner_service = Arc::new(HetznerCredentialService::new(
        credential_repo,
        secret_store.clone(),
    ));

    let cluster_repo: Arc<dyn kubinate_cluster::repository::ClusterRepository> =
        Arc::new(PgClusterRepository::new(pool.clone()));
    let cluster_service = Arc::new(ClusterService::new(
        cluster_repo.clone(),
        secret_store.clone(),
    ));

    let membership_repo: Arc<dyn kubinate_identity::repository::MembershipRepository> =
        Arc::new(PgMembershipRepository::new(pool.clone()));
    let invite_repo: Arc<dyn kubinate_identity::repository::InviteRepository> =
        Arc::new(PgInviteRepository::new(pool.clone()));
    let membership_service = Arc::new(MembershipService::new(invite_repo, membership_repo));

    let ssh_identity = std::env::var("KUBINATE__SSH_KEY_PATH")
        .ok()
        .map(std::path::PathBuf::from);
    let ssh_executor = Arc::new(OpensshExecutor::new(ssh_identity));
    let helm_executor = Arc::new(HelmCliExecutor::new(None));
    let kubectl_executor = Arc::new(KubectlCliExecutor::new(None));
    let event_hub = kubinate_workflows::events::shared_hub();
    let metrics_store: Arc<dyn kubinate_observability::metrics::MetricsStore> = match std::env::var(
        "KUBINATE__METRICS_BACKEND",
    )
    .as_deref()
    .unwrap_or("memory")
    {
        "memory" => Arc::new(kubinate_observability::metrics::InMemoryMetricsStore::new()),
        "victoria_metrics" => {
            let url = std::env::var("KUBINATE__VICTORIA_METRICS_URL").context(
                    "KUBINATE__VICTORIA_METRICS_URL required when KUBINATE__METRICS_BACKEND=victoria_metrics",
                )?;
            tracing::info!(url = %url, "metrics backend: victoria metrics");
            Arc::new(
                kubinate_observability::metrics::VictoriaMetricsStore::new(&url)
                    .context("victoria metrics store init")?,
            )
        }
        other => anyhow::bail!(
            "KUBINATE__METRICS_BACKEND must be 'memory' or 'victoria_metrics'; got '{other}'"
        ),
    };

    // Sprint 4 ticket 05 — WebAuthn opt-in: only build the
    // `Ceremonies` facade if both required env vars are present.
    // Missing config is logged-and-skipped; the rest of the API
    // works unchanged and `auth_passkey` routes return 503.
    let passkey_repo: Arc<dyn kubinate_identity::repository::PasskeyRepository> = Arc::new(
        kubinate_identity::repository::PgPasskeyRepository::new(pool.clone()),
    );
    let recovery_codes_repo: Arc<dyn kubinate_identity::repository::RecoveryCodeRepository> =
        Arc::new(kubinate_identity::repository::PgRecoveryCodeRepository::new(pool.clone()));
    let ceremonies = match kubinate_identity::webauthn::WebauthnConfig::from_env_optional() {
        Ok(Some(cfg)) => {
            let store: Arc<dyn kubinate_identity::webauthn::CeremonyStore> = Arc::new(
                kubinate_identity::webauthn::PgCeremonyStore::new(pool.clone()),
            );
            match kubinate_identity::webauthn::Ceremonies::new(cfg, store) {
                Ok(cer) => {
                    tracing::info!("WebAuthn ceremonies enabled");
                    Some(Arc::new(cer))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "WebAuthn config invalid; passkey routes will return 503");
                    None
                }
            }
        }
        Ok(None) => {
            tracing::info!(
                "WebAuthn not configured (KUBINATE__WEBAUTHN_RP_ID + KUBINATE__WEBAUTHN_RP_ORIGIN); passkey routes will return 503"
            );
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "WebAuthn config invalid; passkey routes will return 503");
            None
        }
    };
    let passkey_pending_nicknames: Arc<
        std::sync::Mutex<std::collections::HashMap<uuid::Uuid, String>>,
    > = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let addon_repo: Arc<dyn kubinate_addons::repository::AddonRepository> = Arc::new(
        kubinate_addons::repository::PgAddonRepository::new(pool.clone()),
    );
    let addon_service = Arc::new(kubinate_addons::service::AddonService::new(
        addon_repo.clone(),
    ));
    let runner = Arc::new(LocalRunner::new(
        pool.clone(),
        cluster_service.clone(),
        cluster_repo.clone(),
        addon_repo,
        ssh_executor,
        helm_executor,
        kubectl_executor,
        event_hub.clone(),
    ));

    let cluster_settings = Arc::new(ClusterSettings {
        ssh_key: std::env::var("KUBINATE__HETZNER_SSH_KEY")
            .unwrap_or_else(|_| "kubinate-operator".to_string()),
        k3s_version: std::env::var("KUBINATE__K3S_VERSION")
            .unwrap_or_else(|_| "v1.30.2+k3s1".to_string()),
        user_data: include_str!("../../../infra/cloud-init/k3s-node.yaml").to_string(),
    });

    let auth_cfg = Arc::new(AuthConfig {
        github_client_id: require_env("KUBINATE__GITHUB_CLIENT_ID")?,
        github_redirect_uri: require_env("KUBINATE__GITHUB_REDIRECT_URI")?,
    });
    let github_secret = SecretString::from(require_env("KUBINATE__GITHUB_CLIENT_SECRET")?);
    let github: SharedGithubClient = Arc::new(HttpGithubClient::new(
        auth_cfg.github_client_id.clone(),
        github_secret,
    ));

    // Stripe billing wiring (Sprint 2 ticket 08). Both env vars must
    // be present in any environment that hits the billing routes; we
    // surface a generic error pre-startup rather than 500-ing later.
    let stripe_secret = SecretString::from(require_env("KUBINATE__STRIPE_SECRET_KEY")?);
    let stripe_webhook_secret = SecretString::from(require_env("KUBINATE__STRIPE_WEBHOOK_SECRET")?);
    let stripe_client: Arc<dyn StripeClient> = Arc::new(HttpStripeClient::new(stripe_secret));
    let billing_repo: Arc<dyn kubinate_billing::repository::BillingRepository> = Arc::new(
        kubinate_billing::repository::PgBillingRepository::new(pool.clone()),
    );
    let billing_service = Arc::new(kubinate_billing::service::BillingService::new(
        billing_repo,
        stripe_client,
        std::env::var("KUBINATE__BILLING_SUCCESS_URL").unwrap_or_else(|_| {
            "https://app.kubinate.com/app/settings/billing?status=success".into()
        }),
        std::env::var("KUBINATE__BILLING_CANCEL_URL").unwrap_or_else(|_| {
            "https://app.kubinate.com/app/settings/billing?status=cancelled".into()
        }),
    ));

    let state = AppState {
        db: pool.clone(),
        started_at: time::OffsetDateTime::now_utc(),
        version: env!("CARGO_PKG_VERSION"),
        hetzner_service,
        cluster_service,
        addon_service,
        membership_service,
        billing_service,
        stripe_webhook_secret,
        cluster_repo,
        secret_store,
        runner,
        github,
        auth: auth_cfg,
        // 5 downloads / 60 seconds / user — DoD row from ticket 04.
        kubeconfig_rate_limit: Arc::new(RateLimiter::new(5, Duration::from_secs(60))),
        cluster_settings,
        event_hub,
        metrics_store,
        ceremonies,
        passkey_repo,
        recovery_codes_repo,
        passkey_pending_nicknames,
    };

    let request_id_header = HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/version", get(version))
        .route("/metrics", get(metrics_endpoint))
        .route("/v1/me", get(me))
        .nest("/v1/auth", auth::routes())
        .nest("/v1/auth/passkey", auth_passkey::routes())
        .nest("/v1/auth/recovery-codes", auth_passkey::recovery_routes())
        .nest("/v1/clusters", clusters::routes())
        .route("/v1/catalog/clusters", get(clusters::catalog))
        .nest("/v1/integrations/hetzner", integrations::routes())
        .nest("/v1/organizations/{org_id}", team::org_routes())
        .nest("/v1/organizations/{org_id}", billing::org_state_route())
        .nest("/v1/invites", team::invite_accept_route())
        .nest("/v1/billing", billing::routes())
        .nest("/v1/observability", observability::routes())
        .with_state(state.clone())
        .layer(SetRequestIdLayer::new(
            request_id_header.clone(),
            MakeRequestUuid,
        ))
        .layer(PropagateRequestIdLayer::new(request_id_header))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(30),
        ));

    let addr: SocketAddr = config.listen_addr.parse().context("listen addr parse")?;
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("bind {addr}"))?;

    agent::spawn_if_enabled(state.cluster_repo.clone(), state.metrics_store.clone());

    tracing::info!(%addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve")?;

    tracing::info!("shutdown complete");
    Ok(())
}

fn require_env(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("missing required env var {name}"))
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    uptime_seconds: i64,
    version: &'static str,
}

async fn healthz(State(state): State<AppState>) -> impl IntoResponse {
    let uptime = (time::OffsetDateTime::now_utc() - state.started_at).whole_seconds();
    Json(Health {
        status: "ok",
        uptime_seconds: uptime,
        version: state.version,
    })
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => (StatusCode::OK, "ready").into_response(),
        Err(err) => {
            tracing::warn!(error = %err, "readiness check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "not ready").into_response()
        }
    }
}

#[derive(Serialize)]
struct Version {
    version: &'static str,
    git_sha: Option<&'static str>,
}

/// `GET /v1/me` — returns the resolved actor + MFA state so the SPA
/// can populate org-aware UIs and route the user to the right MFA
/// flow without inferring state from 401 round-trips.
///
/// `mfa_state` resolves the four-state matrix described on
/// [`kubinate_identity::session::MfaState`]. Sprint 4 ticket 05's
/// SPA hint deferred row + Sprint 5 ticket 07.
async fn me(State(state): State<AppState>, actor: actor::Actor) -> Result<Json<MeView>, ApiError> {
    let mfa_state =
        kubinate_identity::session::mfa_state(&state.db, actor.user_id, actor.session_id).await?;
    Ok(Json(MeView {
        user_id: actor.user_id,
        session_id: actor.session_id,
        organization_id: actor.organization_id,
        mfa_state,
    }))
}

#[derive(Serialize)]
struct MeView {
    user_id: uuid::Uuid,
    session_id: uuid::Uuid,
    organization_id: uuid::Uuid,
    mfa_state: kubinate_identity::session::MfaState,
}

async fn version() -> Json<Version> {
    Json(Version {
        version: env!("CARGO_PKG_VERSION"),
        git_sha: option_env!("GIT_SHA"),
    })
}

/// `GET /metrics` — Prometheus text-format scrape endpoint.
/// Sprint 3 ticket 01 (defer-path) — first consumer is the
/// in-flight runner gauge, but the endpoint stays generic so future
/// gauges + counters land here without a route change.
async fn metrics_endpoint() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4",
        )],
        kubinate_platform::metrics::render(),
    )
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("install ctrl_c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => tracing::info!("ctrl-c received, shutting down"),
        () = terminate => tracing::info!("SIGTERM received, shutting down"),
    }
}
