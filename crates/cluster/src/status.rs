//! User-facing status presentation.
//!
//! Threat-model Flow 3 forbids surfacing raw Hetzner / SSH error
//! strings to the user (they may leak server ids, internal IPs,
//! reflected token fragments). The workflow records a structured
//! reason in `clusters.status_reason`; this module maps those
//! prefixes onto stable categories that the dashboard renders into a
//! friendly message.

use crate::model::ClusterStatus;

/// Stable, opaque-to-the-user categories. The frontend looks up a
/// localised message per category — keep new variants additive so
/// older clients render an unknown variant as a generic error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Hetzner refused the request — usually quota, billing, or
    /// auth. Cosmetically grouped because the user fixes them in the
    /// same place (the Hetzner Console).
    ProviderError,
    /// Network flake — TLS / DNS / transient 5xx. Re-running usually
    /// fixes it.
    NetworkError,
    /// Cloud-init never reported success in time.
    BootTimeout,
    /// Remote command failed with a non-zero exit.
    InstallationError,
    /// Anything we did not bucket explicitly.
    Unknown,
}

impl ErrorCategory {
    /// Stable lower-snake string matching the JSON the API serialises.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCategory::ProviderError => "provider_error",
            ErrorCategory::NetworkError => "network_error",
            ErrorCategory::BootTimeout => "boot_timeout",
            ErrorCategory::InstallationError => "installation_error",
            ErrorCategory::Unknown => "unknown",
        }
    }
}

/// Map the cluster's status + raw reason text onto a category.
/// Returns `None` for non-terminal / non-failed states.
#[must_use]
pub fn error_category(status: ClusterStatus, reason: Option<&str>) -> Option<ErrorCategory> {
    if status != ClusterStatus::Failed {
        return None;
    }
    let reason = reason.unwrap_or("").to_ascii_lowercase();

    // Anchored prefix matching: the workflow writes structured strings
    // like `hetzner: 429 ...` or `ssh: NonZeroExit{...}`. Anchoring
    // avoids accidental matches in free-form messages.
    if reason.starts_with("hetzner:") {
        if reason.contains("401")
            || reason.contains("403")
            || reason.contains("quota")
            || reason.contains("forbidden")
        {
            return Some(ErrorCategory::ProviderError);
        }
        if reason.contains("429")
            || reason.contains("502")
            || reason.contains("503")
            || reason.contains("504")
            || reason.contains("transport")
            || reason.contains("timeout")
        {
            return Some(ErrorCategory::NetworkError);
        }
        return Some(ErrorCategory::ProviderError);
    }
    if reason.starts_with("ssh:") {
        return Some(ErrorCategory::InstallationError);
    }
    if reason.contains("cloud-init") || reason.contains("cloudinittimeout") {
        return Some(ErrorCategory::BootTimeout);
    }
    Some(ErrorCategory::Unknown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_failed_states_have_no_category() {
        for s in [
            ClusterStatus::Pending,
            ClusterStatus::Provisioning,
            ClusterStatus::Ready,
            ClusterStatus::Scaling,
            ClusterStatus::Destroying,
            ClusterStatus::Destroyed,
        ] {
            assert_eq!(error_category(s, Some("hetzner: 429")), None);
        }
    }

    #[test]
    fn hetzner_429_maps_to_network_error() {
        assert_eq!(
            error_category(ClusterStatus::Failed, Some("hetzner: 429 rate limited")),
            Some(ErrorCategory::NetworkError),
        );
    }

    #[test]
    fn hetzner_quota_maps_to_provider_error() {
        assert_eq!(
            error_category(
                ClusterStatus::Failed,
                Some("hetzner: 403 server quota exceeded"),
            ),
            Some(ErrorCategory::ProviderError),
        );
    }

    #[test]
    fn ssh_failure_maps_to_installation_error() {
        assert_eq!(
            error_category(
                ClusterStatus::Failed,
                Some("ssh: NonZeroExit code=42 stderr=k3s install failed"),
            ),
            Some(ErrorCategory::InstallationError),
        );
    }

    #[test]
    fn cloud_init_timeout_maps_to_boot_timeout() {
        assert_eq!(
            error_category(
                ClusterStatus::Failed,
                Some("cloud-init timed out after 60 polls"),
            ),
            Some(ErrorCategory::BootTimeout),
        );
    }

    #[test]
    fn unknown_reason_maps_to_unknown_category() {
        assert_eq!(
            error_category(ClusterStatus::Failed, Some("something we did not bucket")),
            Some(ErrorCategory::Unknown),
        );
    }

    #[test]
    fn empty_reason_on_failed_still_produces_unknown() {
        assert_eq!(
            error_category(ClusterStatus::Failed, None),
            Some(ErrorCategory::Unknown),
        );
    }
}
