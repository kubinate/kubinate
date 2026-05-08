//! `WebAuthn` ceremony layer for Sprint 4 ticket 05.
//!
//! Wraps the `webauthn-rs` crate so the rest of the codebase deals
//! in a small, testable surface:
//!
//! - [`Ceremonies`] — config-driven facade that constructs the
//!   `Webauthn` instance from environment variables and exposes the
//!   four methods the API edge calls (start/finish registration,
//!   start/finish assertion).
//! - [`CeremonyStore`] — trait that persists the in-flight
//!   server-side state between the start and finish RPCs. A
//!   Postgres-backed impl ([`PgCeremonyStore`]) plumbs into the
//!   `webauthn_ceremonies` table; tests use the
//!   [`InMemoryCeremonyStore`].
//!
//! Why a facade rather than direct webauthn-rs calls in the API:
//!
//! 1. **Origin / RP id are environment-driven.** Production runs at
//!    `app.kubinate.com`; staging at `staging.app.kubinate.com`;
//!    dev at `localhost:5173`. The API layer should not have to
//!    know.
//! 2. **State serialisation is delicate.** webauthn-rs gates this
//!    behind the `danger-allow-state-serialisation` feature flag
//!    for a reason — wrong serde shape breaks the ceremony in
//!    silent ways. Keeping it in one place (this module) means the
//!    correct flag + shape only has to be right once.
//! 3. **Ceremony state cleanup**. The store's
//!    [`CeremonyStore::take`] consumes-and-deletes atomically, so
//!    a re-played finish call returns "ceremony not found" rather
//!    than re-finishing.

use std::sync::Arc;

use async_trait::async_trait;
use kubinate_platform::error::PlatformError;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use thiserror::Error;
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;
use webauthn_rs::{
    prelude::{
        Passkey as WebauthnPasskey, PasskeyAuthentication, PasskeyRegistration,
        PublicKeyCredential, RegisterPublicKeyCredential,
    },
    Webauthn, WebauthnBuilder,
};

/// Errors surfaced by the ceremony layer.
#[derive(Debug, Error)]
pub enum WebauthnError {
    /// Configuration: env var missing or malformed (RP id / origin
    /// not parseable as a URL, origin scheme not https outside of
    /// dev). Surfaced at startup, not at request time.
    #[error("webauthn config: {0}")]
    Config(String),
    /// The browser-supplied registration / assertion was rejected by
    /// webauthn-rs. The string is a sanitized reason — never the
    /// underlying crypto error, which can leak authenticator
    /// internals.
    #[error("webauthn ceremony rejected: {0}")]
    Rejected(String),
    /// The ceremony id was unknown or expired. Treated by the API
    /// layer as 410 Gone — the user must restart the flow.
    #[error("ceremony not found or expired")]
    CeremonyMissing,
    /// Storage failure. Logged + surfaced as 500.
    #[error("ceremony storage: {0}")]
    Storage(#[from] PlatformError),
}

/// Discriminator for the two ceremony kinds. Mapped 1:1 with the
/// CHECK constraint in the `webauthn_ceremonies` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CeremonyKind {
    /// Adding a new passkey to an authenticated user.
    Registration,
    /// Verifying an assertion against an existing passkey.
    Assertion,
}

impl CeremonyKind {
    /// Wire-format string matching the table's CHECK constraint.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CeremonyKind::Registration => "registration",
            CeremonyKind::Assertion => "assertion",
        }
    }
}

/// Configuration for the [`Ceremonies`] facade. Read from env at
/// startup; do not construct one per request.
#[derive(Debug, Clone)]
pub struct WebauthnConfig {
    /// Relying-Party ID — the bare hostname users authenticate
    /// against. NOT a URL. Must match the eTLD+1 of every origin.
    pub rp_id: String,
    /// Human-readable RP name shown by the authenticator UI.
    pub rp_name: String,
    /// Allowed origin (scheme + host + port). Browsers compare
    /// against this verbatim.
    pub rp_origin: Url,
}

impl WebauthnConfig {
    /// Load from env if both required vars are set; return `Ok(None)`
    /// if either is missing so the caller can decide to disable
    /// `WebAuthn` rather than fail closed at startup.
    ///
    /// # Errors
    /// Returns [`WebauthnError::Config`] when env vars are present
    /// but malformed (empty RP id, unparseable origin URL).
    pub fn from_env_optional() -> Result<Option<Self>, WebauthnError> {
        match (
            std::env::var("KUBINATE__WEBAUTHN_RP_ID").ok(),
            std::env::var("KUBINATE__WEBAUTHN_RP_ORIGIN").ok(),
        ) {
            (Some(_), Some(_)) => Self::from_env().map(Some),
            _ => Ok(None),
        }
    }

    /// Load from `KUBINATE__WEBAUTHN_RP_ID`,
    /// `KUBINATE__WEBAUTHN_RP_NAME` (defaults `Kubinate`), and
    /// `KUBINATE__WEBAUTHN_RP_ORIGIN`.
    ///
    /// # Errors
    /// Returns [`WebauthnError::Config`] when an env var is
    /// missing, the origin is not a valid URL, or the RP id is
    /// empty.
    pub fn from_env() -> Result<Self, WebauthnError> {
        let rp_id = std::env::var("KUBINATE__WEBAUTHN_RP_ID")
            .map_err(|_| WebauthnError::Config("KUBINATE__WEBAUTHN_RP_ID is required".into()))?;
        if rp_id.trim().is_empty() {
            return Err(WebauthnError::Config(
                "KUBINATE__WEBAUTHN_RP_ID must not be empty".into(),
            ));
        }
        let rp_name =
            std::env::var("KUBINATE__WEBAUTHN_RP_NAME").unwrap_or_else(|_| "Kubinate".to_string());
        let origin_str = std::env::var("KUBINATE__WEBAUTHN_RP_ORIGIN")
            .map_err(|_| WebauthnError::Config("KUBINATE__WEBAUTHN_RP_ORIGIN is required".into()))?;
        let rp_origin = Url::parse(&origin_str).map_err(|e| {
            WebauthnError::Config(format!("KUBINATE__WEBAUTHN_RP_ORIGIN invalid url: {e}"))
        })?;
        Ok(Self {
            rp_id,
            rp_name,
            rp_origin,
        })
    }
}

/// Persisted server-side state for one ceremony in flight.
#[derive(Debug, Clone)]
pub struct CeremonyRow {
    /// Handle returned to the browser; required to call finish.
    pub id: Uuid,
    /// User the ceremony is bound to.
    pub user_id: Uuid,
    /// Kind of ceremony — used to reject cross-kind state.
    pub kind: CeremonyKind,
    /// CBOR-encoded `webauthn-rs` state.
    pub state: Vec<u8>,
    /// Hard expiry; finish calls past this point return
    /// [`WebauthnError::CeremonyMissing`].
    pub expires_at: OffsetDateTime,
}

/// Persistence trait for in-flight ceremony state.
#[async_trait]
pub trait CeremonyStore: Send + Sync {
    /// Insert a freshly-started ceremony. The `id` uniquely
    /// identifies the round-trip; the API layer hands it to the
    /// browser and back.
    async fn put(&self, row: CeremonyRow) -> Result<(), WebauthnError>;

    /// Atomically consume the row by id. Returns `Ok(None)` for
    /// unknown / already-consumed / expired ceremonies. The `kind`
    /// argument is checked: a ceremony stored under one kind cannot
    /// be finished under another.
    async fn take(
        &self,
        id: Uuid,
        kind: CeremonyKind,
    ) -> Result<Option<CeremonyRow>, WebauthnError>;
}

/// Postgres-backed [`CeremonyStore`].
pub struct PgCeremonyStore {
    pool: PgPool,
}

impl PgCeremonyStore {
    /// Wrap a pool for repository use.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CeremonyStore for PgCeremonyStore {
    async fn put(&self, row: CeremonyRow) -> Result<(), WebauthnError> {
        sqlx::query(
            r"
            INSERT INTO webauthn_ceremonies (id, user_id, kind, state, expires_at)
            VALUES ($1, $2, $3, $4, $5)
            ",
        )
        .bind(row.id)
        .bind(row.user_id)
        .bind(row.kind.as_str())
        .bind(&row.state[..])
        .bind(row.expires_at)
        .execute(&self.pool)
        .await
        .map_err(PlatformError::from)?;
        Ok(())
    }

    async fn take(
        &self,
        id: Uuid,
        kind: CeremonyKind,
    ) -> Result<Option<CeremonyRow>, WebauthnError> {
        // DELETE … RETURNING is the single-statement atomic consume:
        // a re-played finish call returns None on the second attempt
        // even under concurrent load.
        let row = sqlx::query(
            r"
            DELETE FROM webauthn_ceremonies
            WHERE id = $1
              AND kind = $2
              AND expires_at > now()
            RETURNING id, user_id, kind, state, expires_at
            ",
        )
        .bind(id)
        .bind(kind.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(PlatformError::from)?;

        Ok(row.map(|r| {
            let kind_str: String = r.get("kind");
            let parsed_kind = match kind_str.as_str() {
                "assertion" => CeremonyKind::Assertion,
                _ => CeremonyKind::Registration, // CHECK constraint blocks others
            };
            CeremonyRow {
                id: r.get("id"),
                user_id: r.get("user_id"),
                kind: parsed_kind,
                state: r.get("state"),
                expires_at: r.get("expires_at"),
            }
        }))
    }
}

/// In-memory [`CeremonyStore`] for unit tests. Drops state on
/// process exit; never use in production.
pub struct InMemoryCeremonyStore {
    inner: std::sync::Mutex<std::collections::HashMap<Uuid, CeremonyRow>>,
}

impl InMemoryCeremonyStore {
    /// Build an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for InMemoryCeremonyStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CeremonyStore for InMemoryCeremonyStore {
    async fn put(&self, row: CeremonyRow) -> Result<(), WebauthnError> {
        self.inner
            .lock()
            .expect("ceremony store poisoned")
            .insert(row.id, row);
        Ok(())
    }

    async fn take(
        &self,
        id: Uuid,
        kind: CeremonyKind,
    ) -> Result<Option<CeremonyRow>, WebauthnError> {
        let mut g = self.inner.lock().expect("ceremony store poisoned");
        match g.remove(&id) {
            Some(row) if row.kind == kind && row.expires_at > OffsetDateTime::now_utc() => {
                Ok(Some(row))
            }
            // Wrong-kind or expired: do not re-insert. The caller
            // gets the same "missing" response either way; the
            // distinction only matters in tracing.
            _ => Ok(None),
        }
    }
}

/// Outcome of a successful registration ceremony — the data the
/// API layer hands to [`crate::repository::PasskeyRepository::insert`].
#[derive(Debug, Clone)]
pub struct CompletedRegistration {
    /// `WebAuthn` credential id (`Base64URL` string).
    pub credential_id: String,
    /// CBOR-encoded `webauthn-rs::Passkey` blob.
    pub credential_blob: Vec<u8>,
}

/// Outcome of a successful assertion ceremony.
#[derive(Debug, Clone)]
pub struct CompletedAssertion {
    /// The id of the passkey row that authenticated. The caller
    /// uses this to update `user_passkeys.last_used_at` +
    /// `sign_counter`.
    pub passkey_row_id: Uuid,
    /// New sign counter from the authenticator. The caller passes
    /// this to
    /// [`crate::repository::PasskeyRepository::record_use`].
    pub new_sign_counter: i64,
    /// Updated `webauthn-rs::Passkey` blob — webauthn-rs requires
    /// us to persist the new state after an assertion (it tracks
    /// counter rollback internally). If the caller persists this
    /// alongside `record_use`, future assertions use the latest.
    pub updated_credential_blob: Vec<u8>,
}

/// The ceremony facade. Construct once at startup; clone the
/// `Arc` into request handlers.
pub struct Ceremonies {
    config: WebauthnConfig,
    webauthn: Webauthn,
    store: Arc<dyn CeremonyStore>,
}

impl Ceremonies {
    /// Build from config + a [`CeremonyStore`].
    ///
    /// # Errors
    /// [`WebauthnError::Config`] if webauthn-rs rejects the
    /// (`rp_id`, `rp_origin`) pair (typically: origin's host doesn't
    /// match `rp_id`).
    pub fn new(
        config: WebauthnConfig,
        store: Arc<dyn CeremonyStore>,
    ) -> Result<Self, WebauthnError> {
        let webauthn = WebauthnBuilder::new(&config.rp_id, &config.rp_origin)
            .map_err(|e| WebauthnError::Config(format!("WebauthnBuilder: {e}")))?
            .rp_name(&config.rp_name)
            .build()
            .map_err(|e| WebauthnError::Config(format!("Webauthn::build: {e}")))?;
        Ok(Self {
            config,
            webauthn,
            store,
        })
    }

    /// Read-only access to the configuration. Useful for tests
    /// that need to assert the values that landed in the facade.
    #[must_use]
    pub fn config(&self) -> &WebauthnConfig {
        &self.config
    }

    /// Open a registration ceremony for the given user. Returns
    /// the ceremony id (handed to the browser as `ceremony_id`)
    /// plus the JSON-serialisable challenge the browser feeds to
    /// `navigator.credentials.create()`.
    ///
    /// `excluded_credentials` is the list of credential ids the
    /// authenticator should refuse to re-register; the API layer
    /// passes the user's existing live passkey credential ids so
    /// a single device isn't enrolled twice.
    ///
    /// # Errors
    /// Wraps any webauthn-rs failure as
    /// [`WebauthnError::Rejected`]; storage failures as
    /// [`WebauthnError::Storage`].
    pub async fn start_registration(
        &self,
        user_id: Uuid,
        user_email: &str,
        user_display_name: &str,
        excluded_credentials: Vec<webauthn_rs::prelude::CredentialID>,
    ) -> Result<(Uuid, webauthn_rs::prelude::CreationChallengeResponse), WebauthnError> {
        let (challenge, registration_state) = self
            .webauthn
            .start_passkey_registration(
                user_id,
                user_email,
                user_display_name,
                Some(excluded_credentials),
            )
            .map_err(|e| WebauthnError::Rejected(format!("start_passkey_registration: {e}")))?;

        let state = serialize_registration_state(&registration_state)?;
        let id = Uuid::now_v7();
        self.store
            .put(CeremonyRow {
                id,
                user_id,
                kind: CeremonyKind::Registration,
                state,
                expires_at: OffsetDateTime::now_utc() + time::Duration::minutes(5),
            })
            .await?;
        Ok((id, challenge))
    }

    /// Finish a registration. Verifies the browser-supplied
    /// `RegisterPublicKeyCredential` against the persisted state;
    /// returns the credential to insert into `user_passkeys`.
    ///
    /// # Errors
    /// [`WebauthnError::CeremonyMissing`] when the id is unknown,
    /// belongs to a different ceremony kind, or has expired;
    /// [`WebauthnError::Rejected`] when the browser response fails
    /// verification.
    pub async fn finish_registration(
        &self,
        ceremony_id: Uuid,
        register: &RegisterPublicKeyCredential,
    ) -> Result<CompletedRegistration, WebauthnError> {
        let row = self
            .store
            .take(ceremony_id, CeremonyKind::Registration)
            .await?
            .ok_or(WebauthnError::CeremonyMissing)?;

        let state = deserialize_registration_state(&row.state)?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(register, &state)
            .map_err(|e| WebauthnError::Rejected(format!("finish_passkey_registration: {e}")))?;

        let credential_id_bytes: &[u8] = passkey.cred_id().as_ref();
        let credential_id = base64_url_encode(credential_id_bytes);
        let credential_blob = serialize_passkey(&passkey)?;
        Ok(CompletedRegistration {
            credential_id,
            credential_blob,
        })
    }

    /// Open an assertion ceremony. The caller passes the user's
    /// live passkeys (deserialized from `user_passkeys.credential`
    /// blobs); webauthn-rs builds an allow-list challenge.
    ///
    /// # Errors
    /// [`WebauthnError::Rejected`] if the user has zero live
    /// passkeys (the caller should redirect to enrollment instead).
    pub async fn start_assertion(
        &self,
        user_id: Uuid,
        passkeys: &[WebauthnPasskey],
    ) -> Result<(Uuid, webauthn_rs::prelude::RequestChallengeResponse), WebauthnError> {
        if passkeys.is_empty() {
            return Err(WebauthnError::Rejected(
                "user has no registered passkeys".into(),
            ));
        }
        let (challenge, auth_state) = self
            .webauthn
            .start_passkey_authentication(passkeys)
            .map_err(|e| WebauthnError::Rejected(format!("start_passkey_authentication: {e}")))?;
        let state = serialize_authentication_state(&auth_state)?;
        let id = Uuid::now_v7();
        self.store
            .put(CeremonyRow {
                id,
                user_id,
                kind: CeremonyKind::Assertion,
                state,
                expires_at: OffsetDateTime::now_utc() + time::Duration::minutes(5),
            })
            .await?;
        Ok((id, challenge))
    }

    /// Finish an assertion. Verifies the browser's
    /// `PublicKeyCredential` against the persisted state. The
    /// caller is responsible for translating the
    /// [`CompletedAssertion::passkey_row_id`] back to the local
    /// `user_passkeys.id` — typically by looking up the user's
    /// passkey whose `credential_id` matches the assertion's
    /// `cred_id`.
    ///
    /// # Errors
    /// [`WebauthnError::CeremonyMissing`] / [`WebauthnError::Rejected`]
    /// per the registration finish.
    pub async fn finish_assertion(
        &self,
        ceremony_id: Uuid,
        passkey_row_id: Uuid,
        passkey: &mut WebauthnPasskey,
        credential: &PublicKeyCredential,
    ) -> Result<CompletedAssertion, WebauthnError> {
        let row = self
            .store
            .take(ceremony_id, CeremonyKind::Assertion)
            .await?
            .ok_or(WebauthnError::CeremonyMissing)?;

        let state = deserialize_authentication_state(&row.state)?;
        let result = self
            .webauthn
            .finish_passkey_authentication(credential, &state)
            .map_err(|e| WebauthnError::Rejected(format!("finish_passkey_authentication: {e}")))?;

        // webauthn-rs may flip the passkey's internal counter; we
        // re-serialize the (potentially mutated) value so the
        // caller can persist it. Counter regression is detected
        // inside `update_credential` — if it returns Some(true) it
        // means the counter advanced; Some(false) means unchanged;
        // None means no update needed for this credential.
        let _ = passkey.update_credential(&result);
        let updated_blob = serialize_passkey(passkey)?;
        let new_counter = i64::from(result.counter());

        Ok(CompletedAssertion {
            passkey_row_id,
            new_sign_counter: new_counter,
            updated_credential_blob: updated_blob,
        })
    }
}

// --- private helpers --------------------------------------------------------

fn serialize_registration_state(state: &PasskeyRegistration) -> Result<Vec<u8>, WebauthnError> {
    serde_json::to_vec(state).map_err(|e| WebauthnError::Storage(PlatformError::Internal(e.into())))
}

fn deserialize_registration_state(bytes: &[u8]) -> Result<PasskeyRegistration, WebauthnError> {
    serde_json::from_slice(bytes)
        .map_err(|e| WebauthnError::Storage(PlatformError::Internal(e.into())))
}

fn serialize_authentication_state(state: &PasskeyAuthentication) -> Result<Vec<u8>, WebauthnError> {
    serde_json::to_vec(state).map_err(|e| WebauthnError::Storage(PlatformError::Internal(e.into())))
}

fn deserialize_authentication_state(bytes: &[u8]) -> Result<PasskeyAuthentication, WebauthnError> {
    serde_json::from_slice(bytes)
        .map_err(|e| WebauthnError::Storage(PlatformError::Internal(e.into())))
}

fn serialize_passkey(passkey: &WebauthnPasskey) -> Result<Vec<u8>, WebauthnError> {
    serde_json::to_vec(passkey)
        .map_err(|e| WebauthnError::Storage(PlatformError::Internal(e.into())))
}

/// Restore a `webauthn-rs::Passkey` from its persisted blob. The
/// API layer's assertion handler calls this once per matching
/// `user_passkeys.credential` row.
///
/// # Errors
/// [`WebauthnError::Storage`] if the blob is corrupt; the caller
/// should treat this as a 500 + log so the operator can audit the
/// row.
pub fn deserialize_passkey(bytes: &[u8]) -> Result<WebauthnPasskey, WebauthnError> {
    serde_json::from_slice(bytes)
        .map_err(|e| WebauthnError::Storage(PlatformError::Internal(e.into())))
}

fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev_config() -> WebauthnConfig {
        WebauthnConfig {
            rp_id: "localhost".to_string(),
            rp_name: "Kubinate (test)".to_string(),
            rp_origin: Url::parse("http://localhost:5173").unwrap(),
        }
    }

    #[test]
    fn ceremony_kind_round_trips_with_table_check_constraint() {
        // The wire-format string MUST match the CHECK constraint
        // from migrations/20260505120100_webauthn_ceremonies.sql.
        // If anyone renames a variant, this test fails before the
        // migration's constraint rejects it at insert time.
        assert_eq!(CeremonyKind::Registration.as_str(), "registration");
        assert_eq!(CeremonyKind::Assertion.as_str(), "assertion");
    }

    #[test]
    fn config_from_env_rejects_empty_rp_id() {
        // `from_env` reads process state, which is hard to isolate
        // in tests without locking. We exercise the validator
        // directly via `WebauthnConfig::from_env` with a temp
        // env var to keep coverage on the empty-string branch.
        // SAFETY: tests in this crate run single-threaded
        // (`#[tokio::test]` without `multi_thread`) so the env
        // mutation doesn't race.
        std::env::set_var("KUBINATE__WEBAUTHN_RP_ID", "  ");
        std::env::set_var("KUBINATE__WEBAUTHN_RP_ORIGIN", "http://localhost:5173");
        let err = WebauthnConfig::from_env().unwrap_err();
        match err {
            WebauthnError::Config(msg) => assert!(msg.contains("must not be empty")),
            other => panic!("unexpected error: {other:?}"),
        }
        std::env::remove_var("KUBINATE__WEBAUTHN_RP_ID");
        std::env::remove_var("KUBINATE__WEBAUTHN_RP_ORIGIN");
    }

    #[tokio::test]
    async fn ceremonies_construction_succeeds_with_dev_config() {
        // Headline: the (rp_id, origin) pair this dev config uses
        // is what `npm run dev` ships with. webauthn-rs's
        // WebauthnBuilder enforces that the origin host matches
        // the rp_id; if someone later changes the front-end port
        // or domain without updating both env vars, this test
        // catches the mismatch on the very first construction.
        let store: Arc<dyn CeremonyStore> = Arc::new(InMemoryCeremonyStore::new());
        let cer = Ceremonies::new(dev_config(), store).expect("dev config builds");
        assert_eq!(cer.config().rp_id, "localhost");
    }

    #[tokio::test]
    async fn ceremonies_rejects_origin_host_mismatch() {
        // Belt + suspenders: an operator who sets
        // KUBINATE__WEBAUTHN_RP_ID=app.kubinate.com but accidentally
        // points KUBINATE__WEBAUTHN_RP_ORIGIN at https://staging…
        // would silently accept assertions from staging in
        // production. webauthn-rs's WebauthnBuilder rejects this;
        // the test asserts the rejection lands rather than
        // discovering it during the first prod login.
        let cfg = WebauthnConfig {
            rp_id: "app.kubinate.com".into(),
            rp_name: "Kubinate".into(),
            rp_origin: Url::parse("https://other-domain.example.com").unwrap(),
        };
        let store: Arc<dyn CeremonyStore> = Arc::new(InMemoryCeremonyStore::new());
        match Ceremonies::new(cfg, store) {
            Err(WebauthnError::Config(_)) => {}
            Err(other) => panic!("unexpected error: {other:?}"),
            Ok(_) => panic!("must reject host/RP-id mismatch"),
        }
    }

    #[tokio::test]
    async fn in_memory_store_consume_is_one_shot() {
        // Round-trip: a stored ceremony is takeable once; the
        // second take returns None. This is the property the
        // assertion replay-protection rides on (Sprint 4 ticket
        // 05 Security DoD row "replay protection").
        let store = InMemoryCeremonyStore::new();
        let id = Uuid::now_v7();
        let user_id = Uuid::now_v7();
        store
            .put(CeremonyRow {
                id,
                user_id,
                kind: CeremonyKind::Registration,
                state: vec![1, 2, 3],
                expires_at: OffsetDateTime::now_utc() + time::Duration::minutes(5),
            })
            .await
            .unwrap();

        let first = store.take(id, CeremonyKind::Registration).await.unwrap();
        assert!(first.is_some());

        let second = store.take(id, CeremonyKind::Registration).await.unwrap();
        assert!(second.is_none(), "second consume must return None");
    }

    #[tokio::test]
    async fn in_memory_store_rejects_wrong_kind() {
        // A registration ceremony id presented to the assertion
        // finish endpoint must not succeed — even if the row is
        // present and unexpired. The CeremonyKind discriminator is
        // the gate.
        let store = InMemoryCeremonyStore::new();
        let id = Uuid::now_v7();
        let user_id = Uuid::now_v7();
        store
            .put(CeremonyRow {
                id,
                user_id,
                kind: CeremonyKind::Registration,
                state: vec![],
                expires_at: OffsetDateTime::now_utc() + time::Duration::minutes(5),
            })
            .await
            .unwrap();
        let result = store.take(id, CeremonyKind::Assertion).await.unwrap();
        assert!(result.is_none(), "wrong-kind take must return None");
    }
}
