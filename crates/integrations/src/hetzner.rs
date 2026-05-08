//! Hetzner Cloud API client.
//!
//! The client holds a [`SecretRef`] to an envelope-encrypted token,
//! never the plaintext. Every outbound request resolves the token via
//! [`SecretStore::get`] inside the method scope and drops it before
//! returning — threat-model Flow 2, ADR-0007.
//!
//! Phase 0 ships only the type, the constructor, and the token-resolve
//! helper. Concrete endpoint wrappers (servers, networks, SSH keys)
//! land in ticket 03 (`ProvisionClusterWorkflow`).

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use kubinate_platform::secrets::{SecretError, SecretRef, SecretStore};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Base URL for the Hetzner Cloud API.
pub const HETZNER_API_BASE: &str = "https://api.hetzner.cloud/v1";

/// Errors surfaced by the Hetzner client.
#[derive(Debug, Error)]
pub enum HetznerError {
    /// The token could not be resolved from the secret store.
    #[error("resolve credential: {0}")]
    Credential(#[from] SecretError),
    /// Low-level transport error.
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    /// Hetzner returned a non-success status.
    #[error("hetzner api status {status}: {body}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// Truncated response body for diagnostics (tokens never appear here).
        body: String,
    },
}

/// Hetzner Cloud API client. Cheap to clone — the [`Arc<dyn SecretStore>`]
/// and [`reqwest::Client`] inside are both reference-counted.
#[derive(Clone)]
pub struct Client {
    store: Arc<dyn SecretStore>,
    handle: SecretRef,
    http: reqwest::Client,
}

impl Client {
    /// Build a client bound to the credential identified by `handle`.
    ///
    /// # Panics
    /// The `reqwest::Client` builder is only infallible on supported
    /// platforms; we unwrap here and rely on integration-test coverage
    /// to catch platform-specific build failures.
    #[must_use]
    pub fn new(store: Arc<dyn SecretStore>, handle: SecretRef) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("kubinate/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest client build");

        Self {
            store,
            handle,
            http,
        }
    }

    /// Which credential this client is bound to.
    #[must_use]
    pub fn handle(&self) -> SecretRef {
        self.handle
    }

    /// Resolve the bound token from the secret store.
    ///
    /// Caller is expected to consume the returned [`SecretString`] and
    /// drop it promptly; the `secrecy` crate zeros the inner bytes on
    /// drop. Intentionally crate-private: external callers go through
    /// higher-level methods (added in ticket 03) rather than touching
    /// plaintext directly.
    pub(crate) async fn resolve_token(&self) -> Result<SecretString, HetznerError> {
        Ok(self.store.get(&self.handle).await?)
    }

    /// Perform an authenticated GET against the Hetzner API and return
    /// the deserialized JSON body.
    pub(crate) async fn get_json<T>(&self, path: &str) -> Result<T, HetznerError>
    where
        T: serde::de::DeserializeOwned,
    {
        let token = self.resolve_token().await?;
        let response = self
            .http
            .get(format!("{HETZNER_API_BASE}{path}"))
            .bearer_auth(token.expose_secret())
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(HetznerError::Status {
                status: status.as_u16(),
                body: truncate(&body, 512),
            });
        }

        Ok(response.json::<T>().await?)
    }

    async fn post_json<B, T>(&self, path: &str, body: &B) -> Result<T, HetznerError>
    where
        B: Serialize + ?Sized,
        T: serde::de::DeserializeOwned,
    {
        let token = self.resolve_token().await?;
        let response = self
            .http
            .post(format!("{HETZNER_API_BASE}{path}"))
            .bearer_auth(token.expose_secret())
            .json(body)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(HetznerError::Status {
                status: status.as_u16(),
                body: truncate(&text, 512),
            });
        }
        Ok(response.json::<T>().await?)
    }

    async fn delete_path(&self, path: &str) -> Result<(), HetznerError> {
        let token = self.resolve_token().await?;
        let response = self
            .http
            .delete(format!("{HETZNER_API_BASE}{path}"))
            .bearer_auth(token.expose_secret())
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(HetznerError::Status {
                status: status.as_u16(),
                body: truncate(&body, 512),
            });
        }
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// HTTP wire shapes for the v1 API.
// -----------------------------------------------------------------------------

#[derive(Serialize)]
struct CreateServerBody<'a> {
    name: &'a str,
    server_type: &'a str,
    location: &'a str,
    image: &'a str,
    ssh_keys: Vec<&'a str>,
    user_data: &'a str,
    start_after_create: bool,
    labels: serde_json::Value,
}

#[derive(Deserialize)]
struct CreateServerResponse {
    server: ServerJson,
}

#[derive(Deserialize)]
struct GetServerResponse {
    server: ServerJson,
}

#[derive(Deserialize)]
struct ServerJson {
    id: u64,
    status: String,
    public_net: PublicNet,
    #[serde(default)]
    private_net: Vec<PrivateNet>,
}

#[derive(Deserialize)]
struct PublicNet {
    ipv4: Option<Ipv4Net>,
}

#[derive(Deserialize)]
struct Ipv4Net {
    ip: Option<String>,
}

#[derive(Deserialize)]
struct PrivateNet {
    ip: Option<String>,
}

impl ServerJson {
    fn into_view(self) -> HetznerServer {
        let status = match self.status.as_str() {
            "running" => ServerStatus::Ready,
            "starting" | "initializing" | "off" | "rebuilding" => ServerStatus::Initializing,
            _ => ServerStatus::Failed,
        };
        HetznerServer {
            id: ServerId(self.id),
            status,
            ipv4: self.public_net.ipv4.and_then(|v| v.ip),
            private_ipv4: self.private_net.into_iter().find_map(|p| p.ip),
        }
    }
}

#[async_trait]
impl HetznerProvider for Client {
    async fn create_server(&self, params: CreateServerParams) -> Result<ServerId, HetznerError> {
        let labels = serde_json::json!({
            "kubinate.idempotency_key": params.idempotency_key.to_string(),
        });
        let body = CreateServerBody {
            name: &params.name,
            server_type: &params.server_type,
            location: &params.location,
            image: "debian-12",
            ssh_keys: vec![&params.ssh_key],
            user_data: &params.user_data,
            start_after_create: true,
            labels,
        };
        let response: CreateServerResponse = self.post_json("/servers", &body).await?;
        Ok(ServerId(response.server.id))
    }

    async fn get_server(&self, id: ServerId) -> Result<HetznerServer, HetznerError> {
        let response: GetServerResponse = self.get_json(&format!("/servers/{}", id.0)).await?;
        Ok(response.server.into_view())
    }

    async fn delete_server(&self, id: ServerId) -> Result<(), HetznerError> {
        self.delete_path(&format!("/servers/{}", id.0)).await
    }
}

/// Hetzner Cloud server id. Typed for clarity at activity boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerId(pub u64);

/// Lifecycle state of a Hetzner server we care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerStatus {
    /// Server is booted and cloud-init has reported success.
    Ready,
    /// Server exists but cloud-init has not finished yet.
    Initializing,
    /// Terminal error state — compensate.
    Failed,
}

/// Just-enough server view for the provisioning workflow.
#[derive(Debug, Clone)]
pub struct HetznerServer {
    /// Id issued by Hetzner.
    pub id: ServerId,
    /// Current status.
    pub status: ServerStatus,
    /// Public IPv4 address (populated once the server is booted).
    pub ipv4: Option<String>,
    /// Private network IP inside the cluster VPC.
    pub private_ipv4: Option<String>,
}

/// Parameters for [`HetznerProvider::create_server`].
#[derive(Debug, Clone)]
pub struct CreateServerParams {
    /// Hostname.
    pub name: String,
    /// Hetzner server type slug (e.g. `cpx21`).
    pub server_type: String,
    /// Hetzner location slug (e.g. `nbg1`).
    pub location: String,
    /// SSH key fingerprint or id registered in the project; the
    /// activity passes this through so test mocks can assert on it.
    pub ssh_key: String,
    /// cloud-init `#cloud-config` document.
    pub user_data: String,
    /// A correlation id used by the caller to deduplicate retries.
    /// Passed through to Hetzner as a label so accidental duplicate
    /// create calls can be detected in Cloud Console audit.
    pub idempotency_key: Uuid,
}

/// Trait boundary the provisioning workflow uses for Hetzner
/// operations. A real HTTP implementation on top of [`Client`] lands
/// alongside the feature-flagged integration test (ticket 03 DoD).
#[async_trait]
pub trait HetznerProvider: Send + Sync {
    /// Create a server. The returned [`ServerId`] may refer to a
    /// server that has not yet booted — the caller typically follows
    /// up with `wait_until_ready`.
    async fn create_server(&self, params: CreateServerParams) -> Result<ServerId, HetznerError>;

    /// Fetch server metadata — used to poll cloud-init completion.
    async fn get_server(&self, id: ServerId) -> Result<HetznerServer, HetznerError>;

    /// Destroy a server (compensation path; used by the destroy
    /// workflow in ticket 05).
    async fn delete_server(&self, id: ServerId) -> Result<(), HetznerError>;
}

#[allow(dead_code)] // used by get_json; see ticket 03
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use uuid::Uuid;

    /// In-memory `SecretStore` that records get-calls so the test can
    /// assert the client routes through the store rather than holding
    /// plaintext itself.
    struct MockStore {
        plaintext: String,
        get_calls: AtomicUsize,
    }

    #[async_trait]
    impl SecretStore for MockStore {
        async fn put(
            &self,
            organization_id: Uuid,
            _plaintext: SecretString,
        ) -> Result<SecretRef, SecretError> {
            Ok(SecretRef {
                id: Uuid::now_v7(),
                organization_id,
            })
        }

        async fn get(&self, handle: &SecretRef) -> Result<SecretString, SecretError> {
            self.get_calls.fetch_add(1, Ordering::SeqCst);
            let _ = handle;
            Ok(SecretString::from(self.plaintext.clone()))
        }

        async fn delete(&self, _handle: &SecretRef) -> Result<(), SecretError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn resolve_token_routes_through_secret_store() {
        let store = Arc::new(MockStore {
            plaintext: "hcloud_ut_token".to_string(),
            get_calls: AtomicUsize::new(0),
        });
        let handle = SecretRef {
            id: Uuid::now_v7(),
            organization_id: Uuid::now_v7(),
        };
        let client = Client::new(store.clone(), handle);

        let token = client.resolve_token().await.expect("resolve");
        assert_eq!(token.expose_secret(), "hcloud_ut_token");
        assert_eq!(store.get_calls.load(Ordering::SeqCst), 1);
    }
}
