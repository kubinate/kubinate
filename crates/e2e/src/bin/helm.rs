//! Sprint 3 ticket 05 — kind-cluster Helm runner.
//!
//! Runs the production [`install_addon`] workflow path with the real
//! `HelmCliExecutor` against a Kubernetes cluster that the caller
//! has already brought up and handed us a kubeconfig for. Used by
//! `.github/workflows/ci-helm.yml`; nothing in here requires a
//! Hetzner project, an SSH key, or a Postgres pool.
//!
//! Usage:
//! ```text
//! kubinate-e2e-helm \
//!   --kubeconfig /tmp/kind-kubeconfig.yaml \
//!   --addon ingress-nginx \
//!   --version 4.10.0
//! ```
//!
//! Exit codes:
//! - `0`: Helm reported success and every Pod in the addon's
//!   namespace reached `Ready` within the timeout.
//! - non-zero: anything else; the CI job is responsible for
//!   collecting `kubectl describe` / `kubectl logs` output before
//!   the runner is torn down.
//!
//! Why a separate binary instead of a `cargo test`: this is meant to
//! be the last leg of a CI matrix step that has already paid for a
//! kind cluster, helm, and kubectl. Keeping it as a dedicated binary
//! keeps the test harness composable (the CI job can run multiple
//! addons sequentially in the same kind cluster) and lets the
//! workspace's `cargo test` continue to be hermetic.

#![forbid(unsafe_code)]

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::{bail, Context};
use kubinate_addons::catalog;
use kubinate_integrations::helm::{HelmCliExecutor, InstallParams};
use kubinate_workflows::workflows::{
    install_addon, InstallAddonDeps, InstallAddonInput, NoopProgress,
};

/// Per-pod readiness wait. Helm itself returns long before the
/// chart's resulting Pods come up, so we have to poll. Seven minutes
/// is comfortable for the heaviest catalog entry today (cert-manager
/// pulls four images on a cold node) and well inside the CI job's
/// wallclock budget.
const POD_READY_TIMEOUT: Duration = Duration::from_secs(7 * 60);
/// Inner kubectl call timeout — short enough that a hung apiserver
/// surfaces quickly rather than eating the outer budget.
const KUBECTL_CALL_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug)]
struct Args {
    kubeconfig: PathBuf,
    addon: String,
    version: String,
}

fn parse_args() -> anyhow::Result<Args> {
    let mut kubeconfig: Option<PathBuf> = None;
    let mut addon: Option<String> = None;
    let mut version: Option<String> = None;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--kubeconfig" => kubeconfig = iter.next().map(PathBuf::from),
            "--addon" => addon = iter.next(),
            "--version" => version = iter.next(),
            "-h" | "--help" => {
                println!(
                    "Usage: kubinate-e2e-helm --kubeconfig <path> --addon <slug> --version <ver>"
                );
                std::process::exit(0);
            }
            other => bail!("unknown argument: {other}"),
        }
    }
    Ok(Args {
        kubeconfig: kubeconfig
            .or_else(|| std::env::var_os("KUBECONFIG").map(PathBuf::from))
            .context("--kubeconfig is required (or set KUBECONFIG)")?,
        addon: addon.context("--addon is required")?,
        version: version.context("--version is required")?,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let args = parse_args()?;

    if !args.kubeconfig.exists() {
        bail!(
            "kubeconfig path does not exist: {}",
            args.kubeconfig.display()
        );
    }
    let spec = catalog::lookup(&args.addon)
        .with_context(|| format!("addon {} is not in the catalog", args.addon))?;

    tracing::info!(
        addon = %spec.slug,
        version = %args.version,
        kubeconfig = %args.kubeconfig.display(),
        "starting helm install against kind"
    );

    // Sprint 2 ticket 07's helm executor — same code-path the
    // production runner takes. The `install_addon` workflow function
    // does the namespace creation + idempotent upgrade.
    let helm = Arc::new(HelmCliExecutor::new(None));
    let deps = InstallAddonDeps {
        helm: helm.clone(),
        progress: Arc::new(NoopProgress),
    };
    let params = InstallParams {
        release: spec.release_name.to_string(),
        chart: spec.chart_name.to_string(),
        repo: spec.chart_repo.to_string(),
        version: args.version.clone(),
        namespace: spec.namespace.to_string(),
        values_yaml: spec.default_values_yaml.to_string(),
    };
    install_addon(
        &deps,
        InstallAddonInput {
            params: params.clone(),
            kubeconfig_path: args.kubeconfig.clone(),
        },
    )
    .await
    .with_context(|| format!("install_addon failed for {}", spec.slug))?;
    tracing::info!(addon = %spec.slug, "helm install reported success");

    wait_for_pods_ready(&args.kubeconfig, spec.namespace, POD_READY_TIMEOUT)
        .await
        .with_context(|| format!("pods in {} did not reach Ready", spec.namespace))?;
    tracing::info!(
        namespace = %spec.namespace,
        "all pods reached Ready"
    );

    println!(
        "OK: addon {} v{} ready in {}",
        spec.slug, args.version, spec.namespace
    );
    Ok(())
}

/// Block until every Pod in the namespace is `Ready=True`, or the
/// deadline elapses. Polls via `kubectl wait` rather than implementing
/// a kube client here — the kind job already has kubectl, and the
/// Sprint 1/2 design (CONTRIBUTING.md §"avoid hand-rolled HTTP
/// clients") prefers shelling out for one-shot tools.
async fn wait_for_pods_ready(
    kubeconfig: &std::path::Path,
    namespace: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + timeout;

    // First wait for at least one pod to exist — `kubectl wait` with
    // `--all` returns immediately on an empty namespace and exits 0,
    // which is the wrong answer.
    loop {
        if std::time::Instant::now() >= deadline {
            bail!("timed out waiting for any pod to appear in namespace {namespace}");
        }
        let count = kubectl_pod_count(kubeconfig, namespace).await?;
        if count > 0 {
            tracing::info!(namespace, count, "pods present, waiting for Ready");
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    let secs = remaining.as_secs().max(1);
    let mut cmd = tokio::process::Command::new("kubectl");
    cmd.env("KUBECONFIG", kubeconfig)
        .args([
            "wait",
            "--for=condition=Ready",
            "pod",
            "--all",
            "-n",
            namespace,
            &format!("--timeout={secs}s"),
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(remaining + Duration::from_secs(5), cmd.output())
        .await
        .context("kubectl wait outer timeout")?
        .context("kubectl wait spawn")?;
    if !output.status.success() {
        bail!(
            "kubectl wait failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

async fn kubectl_pod_count(kubeconfig: &std::path::Path, namespace: &str) -> anyhow::Result<usize> {
    let mut cmd = tokio::process::Command::new("kubectl");
    cmd.env("KUBECONFIG", kubeconfig)
        .args([
            "get",
            "pods",
            "-n",
            namespace,
            "--no-headers",
            "--ignore-not-found",
        ])
        .kill_on_drop(true);
    let output = tokio::time::timeout(KUBECTL_CALL_TIMEOUT, cmd.output())
        .await
        .context("kubectl get pods timed out")?
        .context("kubectl get pods spawn")?;
    if !output.status.success() {
        bail!(
            "kubectl get pods failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count())
}
