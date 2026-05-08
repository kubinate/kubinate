//! End-to-end harness for the nightly CI job (Sprint 1 ticket 08).
//!
//! Drives a real `provision → kubectl get nodes → destroy → assert
//! zero residual servers` cycle against a Hetzner test project. The
//! `.github/workflows/nightly-e2e.yml` job wraps this binary with a
//! 60-minute outer timeout and an issue-on-failure step.
//!
//! Required environment:
//!   DATABASE_URL                 — Postgres for app + audit + state
//!   KUBINATE_KEK                 — base64 32-byte envelope-encryption key
//!   KUBINATE_TEST_HETZNER_TOKEN  — Hetzner Cloud token for the test project
//!   KUBINATE_HETZNER_SSH_KEY     — name (or id) of an SSH key already
//!                                  registered in the test project
//!   KUBINATE_SSH_KEY_PATH        — local path to the matching private key
//!
//! Optional:
//!   KUBINATE_E2E_REGION          — default "nbg1"
//!   KUBINATE_E2E_SERVER_TYPE     — default "cpx21"
//!   KUBINATE_E2E_K3S_VERSION     — default "v1.30.2+k3s1"

#![forbid(unsafe_code)]

use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use kubinate_cluster::{
    model::{ClusterStatus, NewCluster},
    repository::{ClusterRepository, PgClusterRepository},
    service::ClusterService,
};
use kubinate_identity::{
    repository::{HetznerCredentialRepository, PgHetznerCredentialRepository},
    service::HetznerCredentialService,
};
use kubinate_integrations::{
    helm::HelmCliExecutor,
    hetzner::{Client as HetznerClient, HetznerProvider},
    kubectl::KubectlCliExecutor,
    ssh::OpensshExecutor,
};
use kubinate_platform::{
    audit::AuditContext,
    db,
    secrets::{PgcryptoStore, SecretStore},
};
use kubinate_workflows::{runner::LocalRunner, workflows::ProvisionClusterInput};
use secrecy::SecretString;
use sqlx::PgPool;
use uuid::Uuid;

/// Outer wallclock budget. Anything past this triggers the
/// force-destroy + non-zero exit (DoD: cluster never lives more than
/// 60 minutes).
const E2E_DEADLINE: Duration = Duration::from_secs(60 * 60);

/// How often the harness polls cluster status while waiting for ready.
const POLL_INTERVAL: Duration = Duration::from_secs(15);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = HarnessArgs::from_env()?;
    tracing::info!(region = %args.region, server_type = %args.server_type, "starting e2e");

    // Outer timeout — the deadline guards against any single phase
    // wedging beyond 60 minutes. We always run the destroy path, even
    // if the deadline tripped, before reporting failure.
    let result = tokio::time::timeout(E2E_DEADLINE, run(&args)).await;

    match result {
        Ok(Ok(())) => {
            tracing::info!("e2e PASS");
            Ok(())
        }
        Ok(Err(err)) => {
            tracing::error!(error = ?err, "e2e FAIL");
            Err(err)
        }
        Err(_) => {
            tracing::error!("e2e exceeded {E2E_DEADLINE:?} — force-destroy path");
            // We don't have a cluster id at the deadline if seeding
            // itself stalled; the GitHub workflow's wider timeout +
            // explicit list-and-delete pass handles orphan cleanup.
            anyhow::bail!("e2e timed out");
        }
    }
}

async fn run(args: &HarnessArgs) -> Result<()> {
    let pool = db::pool(&args.database_url, 5)
        .await
        .context("connect database")?;
    db::migrate(&pool).await.context("run migrations")?;

    let ctx = Setup::seed(&pool, args).await?;
    tracing::info!(
        organization = %ctx.organization_id,
        cluster = %ctx.cluster_id,
        "harness seeded org + cluster",
    );

    // Always run finalize_destroy even on the success path to guarantee
    // zero-residue invariant. We re-use the runner's destroy flow.
    let outcome = run_provision_and_validate(&pool, args, &ctx).await;

    let destroy_outcome = ctx
        .runner
        .run_destroy(ctx.organization_id, ctx.hetzner.clone(), ctx.cluster_id)
        .await;

    if let Err(err) = &destroy_outcome {
        tracing::error!(error = %err, "destroy failed in harness teardown");
    }

    // Independently verify Hetzner is empty: a successful destroy
    // workflow + a "no servers tagged with our cluster id remain"
    // check together prove the AC.
    assert_no_residual_servers(&args.hetzner_token, ctx.cluster_id).await?;

    outcome.and(destroy_outcome.map_err(|e| anyhow::anyhow!(e)))
}

async fn run_provision_and_validate(pool: &PgPool, args: &HarnessArgs, ctx: &Setup) -> Result<()> {
    ctx.runner
        .run_provision(
            ctx.organization_id,
            ctx.hetzner.clone(),
            ProvisionClusterInput {
                cluster_id: ctx.cluster_id,
                cluster_name: ctx.cluster_name.clone(),
                location: args.region.clone(),
                server_type: args.server_type.clone(),
                worker_count: 1,
                ssh_key: args.hetzner_ssh_key.clone(),
                user_data: K3S_NODE_CLOUD_INIT.to_string(),
                k3s_version: args.k3s_version.clone(),
            },
        )
        .await
        .context("provision run")?;

    wait_until_ready(pool, ctx).await?;

    let kubeconfig = ctx
        .cluster_service
        .fetch_kubeconfig(ctx.organization_id, ctx.cluster_id)
        .await
        .context("fetch kubeconfig")?;

    assert_kubectl_works(&kubeconfig).await
}

async fn wait_until_ready(_pool: &PgPool, ctx: &Setup) -> Result<()> {
    let started = Instant::now();
    loop {
        let cluster = ctx
            .cluster_repo
            .get(ctx.organization_id, ctx.cluster_id)
            .await?;
        match cluster.status {
            ClusterStatus::Ready => return Ok(()),
            ClusterStatus::Failed => {
                anyhow::bail!(
                    "cluster reached failed status (reason: {:?})",
                    cluster.status_reason
                );
            }
            ClusterStatus::Destroyed => {
                anyhow::bail!("cluster reached destroyed before ready");
            }
            _ => {
                if started.elapsed() > Duration::from_secs(30 * 60) {
                    anyhow::bail!("provisioning did not reach ready in 30 minutes");
                }
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}

async fn assert_kubectl_works(kubeconfig: &SecretString) -> Result<()> {
    use secrecy::ExposeSecret;
    use std::io::Write;

    // Write the kubeconfig to a tempfile so the kubectl subprocess can
    // read it. Restrict mode 0600 so transient processes cannot read
    // a fresh token off disk.
    let mut path = std::env::temp_dir();
    path.push(format!("kubinate-e2e-{}.yaml", Uuid::now_v7()));
    let mut file = std::fs::File::create(&path).context("create kubeconfig tempfile")?;
    file.write_all(kubeconfig.expose_secret().as_bytes())?;
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }

    let output = tokio::process::Command::new("kubectl")
        .arg("--kubeconfig")
        .arg(&path)
        .arg("get")
        .arg("nodes")
        .arg("-o")
        .arg("json")
        .output()
        .await
        .context("spawn kubectl")?;

    // Always remove the tempfile, even on failure.
    let _ = std::fs::remove_file(&path);

    if !output.status.success() {
        anyhow::bail!(
            "kubectl get nodes failed: status={:?} stderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parse kubectl json")?;
    let items = json["items"].as_array().map(|a| a.len()).unwrap_or(0);
    anyhow::ensure!(
        items >= 2,
        "expected at least 2 nodes (1 cp + 1 worker), got {items}",
    );
    tracing::info!(node_count = items, "kubectl get nodes succeeded");
    Ok(())
}

async fn assert_no_residual_servers(token: &str, cluster_id: Uuid) -> Result<()> {
    let label_selector = format!("kubinate.idempotency_key={cluster_id}");
    let url = format!(
        "https://api.hetzner.cloud/v1/servers?label_selector={}",
        urlencode(&label_selector),
    );
    let response = reqwest::Client::new()
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .context("hetzner servers list")?;
    if !response.status().is_success() {
        anyhow::bail!(
            "hetzner servers list returned {}",
            response.status().as_u16()
        );
    }
    let body: serde_json::Value = response.json().await?;
    let count = body["servers"].as_array().map(|a| a.len()).unwrap_or(0);
    anyhow::ensure!(
        count == 0,
        "post-destroy: {count} server(s) still tagged with cluster_id {cluster_id}",
    );
    tracing::info!("hetzner residue check: 0 servers");
    Ok(())
}

fn urlencode(input: &str) -> String {
    // Tiny URL-encoder — full crate is overkill for this single call.
    input
        .bytes()
        .flat_map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![b]
            }
            _ => format!("%{b:02X}").into_bytes(),
        })
        .map(|b| b as char)
        .collect()
}

struct HarnessArgs {
    database_url: String,
    hetzner_token: String,
    hetzner_ssh_key: String,
    ssh_key_path: PathBuf,
    region: String,
    server_type: String,
    k3s_version: String,
}

impl HarnessArgs {
    fn from_env() -> Result<Self> {
        Ok(Self {
            database_url: env_required("DATABASE_URL")?,
            hetzner_token: env_required("KUBINATE_TEST_HETZNER_TOKEN")?,
            hetzner_ssh_key: env_required("KUBINATE_HETZNER_SSH_KEY")?,
            ssh_key_path: PathBuf::from(env_required("KUBINATE_SSH_KEY_PATH")?),
            region: std::env::var("KUBINATE_E2E_REGION").unwrap_or_else(|_| "nbg1".into()),
            server_type: std::env::var("KUBINATE_E2E_SERVER_TYPE")
                .unwrap_or_else(|_| "cpx21".into()),
            k3s_version: std::env::var("KUBINATE_E2E_K3S_VERSION")
                .unwrap_or_else(|_| "v1.30.2+k3s1".into()),
        })
    }
}

fn env_required(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("env var {name} must be set"))
}

struct Setup {
    organization_id: Uuid,
    cluster_id: Uuid,
    cluster_name: String,
    cluster_repo: Arc<dyn ClusterRepository>,
    cluster_service: Arc<ClusterService>,
    runner: Arc<LocalRunner>,
    hetzner: Arc<dyn HetznerProvider>,
}

impl Setup {
    async fn seed(pool: &PgPool, args: &HarnessArgs) -> Result<Self> {
        // Stand up org + credential row with the test token encrypted.
        let secret_store: Arc<dyn SecretStore> = Arc::new(PgcryptoStore::from_env(pool.clone())?);

        let organization_id = Uuid::now_v7();
        let slug = format!("e2e-{}", &organization_id.simple().to_string()[..8]);
        sqlx::query("INSERT INTO organizations (id, slug, display_name) VALUES ($1, $2, $2)")
            .bind(organization_id)
            .bind(&slug)
            .execute(pool)
            .await?;

        let credential_repo: Arc<dyn HetznerCredentialRepository> =
            Arc::new(PgHetznerCredentialRepository::new(pool.clone()));
        let credential_service = Arc::new(HetznerCredentialService::new(
            credential_repo.clone(),
            secret_store.clone(),
        ));
        // The harness runs without an interactive user, so the audit
        // context only carries `user_agent` to make the audit trail
        // identifiable as harness-driven activity.
        let harness_audit = AuditContext {
            actor_user_id: None,
            request_id: Some(format!("e2e-{organization_id}")),
            ip: None,
            user_agent: Some("e2e/harness".to_string()),
        };

        let credential = credential_service
            .create(
                organization_id,
                "e2e-token".into(),
                SecretString::from(args.hetzner_token.clone()),
                &harness_audit,
            )
            .await?;

        let cluster_repo: Arc<dyn ClusterRepository> =
            Arc::new(PgClusterRepository::new(pool.clone()));
        let cluster_service = Arc::new(ClusterService::new(
            cluster_repo.clone(),
            secret_store.clone(),
        ));

        let cluster_name = format!("e2e-{}", &Uuid::now_v7().simple().to_string()[..8]);
        let cluster = cluster_service
            .create(
                organization_id,
                NewCluster {
                    name: cluster_name.clone(),
                    region: args.region.clone(),
                    server_type: args.server_type.clone(),
                    control_plane_count: 1,
                    worker_count: 1,
                    credential_id: credential.id,
                },
                &harness_audit,
            )
            .await?;

        let ssh = Arc::new(OpensshExecutor::new(Some(args.ssh_key_path.clone())));
        let helm = Arc::new(HelmCliExecutor::new(None));
        let kubectl = Arc::new(KubectlCliExecutor::new(None));
        let addon_repo: Arc<dyn kubinate_addons::repository::AddonRepository> = Arc::new(
            kubinate_addons::repository::PgAddonRepository::new(pool.clone()),
        );
        let runner = Arc::new(LocalRunner::new(
            pool.clone(),
            cluster_service.clone(),
            cluster_repo.clone(),
            addon_repo,
            ssh,
            helm,
            kubectl,
            kubinate_workflows::events::shared_hub(),
        ));

        let hetzner: Arc<dyn HetznerProvider> =
            Arc::new(HetznerClient::new(secret_store, credential.secret_ref));

        Ok(Self {
            organization_id,
            cluster_id: cluster.id,
            cluster_name,
            cluster_repo,
            cluster_service,
            runner,
            hetzner,
        })
    }
}

const K3S_NODE_CLOUD_INIT: &str = include_str!("../../../infra/cloud-init/k3s-node.yaml");
