//! Allowlist of installable add-ons.
//!
//! Sprint 2 ticket 07 shipped the first one (ingress-nginx); Sprint 3
//! ticket 09 added cert-manager. Loki, Velero, ArgoCD follow in later
//! sprints. Adding a new entry here is the only change required for
//! the install workflow to accept it — the chart repo and default
//! release name are derived from the slug.
//!
//! The allowlist is the user-facing security boundary: any chart not
//! listed here is rejected before the workflow even starts, so a forged
//! request body can't trick us into installing arbitrary Helm charts on
//! a customer's k3s.

use kubinate_platform::error::PlatformError;

/// One entry in the catalog. Keep this struct small — the workflow
/// only needs the slug + chart coordinates + a default values blob.
#[derive(Debug, Clone, Copy)]
pub struct AddonSpec {
    /// User-facing slug. Stable; the API accepts this string.
    pub slug: &'static str,
    /// Helm chart repo URL.
    pub chart_repo: &'static str,
    /// Helm chart name within that repo.
    pub chart_name: &'static str,
    /// Helm release name to use inside the cluster.
    pub release_name: &'static str,
    /// Kubernetes namespace to install into. Created if missing.
    pub namespace: &'static str,
    /// Default Helm `--values` document. Empty `""` means defaults.
    pub default_values_yaml: &'static str,
}

const CATALOG: &[AddonSpec] = &[
    AddonSpec {
        slug: "ingress-nginx",
        chart_repo: "https://kubernetes.github.io/ingress-nginx",
        chart_name: "ingress-nginx",
        release_name: "ingress-nginx",
        namespace: "ingress-nginx",
        // Single-replica controller for Sprint 1's single-node CP.
        // HA is Sprint 3+ territory.
        default_values_yaml: "controller:\n  replicaCount: 1\n  service:\n    type: NodePort\n",
    },
    AddonSpec {
        slug: "cert-manager",
        chart_repo: "https://charts.jetstack.io",
        chart_name: "cert-manager",
        release_name: "cert-manager",
        namespace: "cert-manager",
        // `crds.enabled: true` is the modern (v1.15+) replacement for
        // the older `installCRDs: true` knob. The chart installs CRDs
        // *before* the controller, which is exactly the ordering we
        // need — manual `kubectl apply -f crds.yaml` would race the
        // controller's initial reconcile. See
        // docs/runbooks/addon-install-stuck.md for the failure mode if
        // this ever gets flipped off.
        default_values_yaml: "crds:\n  enabled: true\n",
    },
];

/// Look up a catalog entry by slug.
///
/// # Errors
/// [`PlatformError::Invalid`] when the slug isn't on the allowlist.
pub fn lookup(slug: &str) -> Result<&'static AddonSpec, PlatformError> {
    CATALOG
        .iter()
        .find(|s| s.slug == slug)
        .ok_or_else(|| PlatformError::Invalid(format!("unknown addon: {slug}")))
}

/// Slugs of every catalog entry — the API exposes this so the
/// frontend can render a picker without baking the list in twice.
#[must_use]
pub fn slugs() -> Vec<&'static str> {
    CATALOG.iter().map(|s| s.slug).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_nginx_is_on_the_allowlist() {
        let spec = lookup("ingress-nginx").expect("ingress-nginx in catalog");
        assert_eq!(spec.namespace, "ingress-nginx");
        assert!(spec.chart_repo.starts_with("https://"));
    }

    #[test]
    fn unknown_addon_is_rejected() {
        let err = lookup("definitely-not-real").unwrap_err();
        assert!(matches!(err, PlatformError::Invalid(_)));
    }

    #[test]
    fn slugs_includes_ingress_nginx() {
        assert!(slugs().contains(&"ingress-nginx"));
    }

    #[test]
    fn cert_manager_is_on_the_allowlist() {
        let spec = lookup("cert-manager").expect("cert-manager in catalog");
        assert_eq!(spec.namespace, "cert-manager");
        assert!(spec.chart_repo.starts_with("https://"));
        // The CRD-enable knob is load-bearing — the runbook
        // explicitly references it.
        assert!(spec.default_values_yaml.contains("crds:"));
        assert!(spec.default_values_yaml.contains("enabled: true"));
    }

    #[test]
    fn catalog_slugs_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for slug in slugs() {
            assert!(seen.insert(slug), "duplicate slug in catalog: {slug}");
        }
    }
}
