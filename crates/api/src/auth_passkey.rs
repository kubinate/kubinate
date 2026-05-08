//! `/v1/auth/passkey/*` — Sprint 4 ticket 05 endpoints.
//!
//! Six handlers, two ceremony pairs:
//!
//! - **Registration** (`register/start` + `register/finish`): an
//!   already-authenticated user adds a new passkey. Requires a full
//!   `Actor` — partial-MFA sessions must complete the assertion path
//!   first (otherwise a stolen partial session could enroll a
//!   credential the legitimate user can't see).
//! - **Assertion** (`assert/start` + `assert/finish`): the
//!   partial-MFA session's gateway. Uses [`SessionUser`] (no org
//!   binding required) so a user with an MFA-required role but no
//!   active org context still completes the gate.
//! - **Inventory** (`list` + revoke): full-`Actor`-only, drives the
//!   `/app/settings/security` page.
//!
//! Route-level enforcement (gating Owner/Admin operations on
//! `Session.mfa_satisfied`) is **not** wired in this commit — the
//! `OwnerActor` extractor + the migration of existing routes wants
//! a separate review pass. For now: registering a passkey is opt-in
//! per user; the OAuth callback still issues full sessions.
//!
//! WebAuthn opt-in: if `KUBINATE_WEBAUTHN_RP_ID` /
//! `KUBINATE_WEBAUTHN_RP_ORIGIN` aren't set, every handler returns
//! 503 with a Problem Details body. The rest of the API works
//! unchanged. This keeps existing dev / prod deploys functional
//! while operators roll out the WebAuthn config.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use kubinate_identity::{
    recovery_codes as rc,
    session,
    webauthn::{Ceremonies, CompletedAssertion, CompletedRegistration, WebauthnError},
};
use kubinate_platform::error::PlatformError;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, CredentialID, PublicKeyCredential, RegisterPublicKeyCredential,
    RequestChallengeResponse,
};

use crate::{actor::Actor, actor::SessionUser, problem::ApiError, AppState};

/// Mount the passkey routes.
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/register/start", post(register_start))
        .route("/register/finish", post(register_finish))
        .route("/assert/start", post(assert_start))
        .route("/assert/finish", post(assert_finish))
        .route("/list", get(list))
        .route("/:id", delete(revoke))
}

/// Mount the recovery-code routes (peer to passkey routes; same
/// MFA flow). Mounted under `/v1/auth/recovery-codes` in main.rs.
pub fn recovery_routes() -> Router<AppState> {
    Router::new()
        .route("/regenerate", post(recovery_regenerate))
        .route("/redeem", post(recovery_redeem))
}

// --- registration ----------------------------------------------------------

#[derive(Deserialize)]
struct RegisterStartRequest {
    /// User-facing label for the new passkey ("YubiKey 5C").
    nickname: String,
}

#[derive(Serialize)]
struct RegisterStartResponse {
    /// Handle the browser POSTs back to register/finish.
    ceremony_id: Uuid,
    /// JSON the browser hands to `navigator.credentials.create()`.
    challenge: CreationChallengeResponse,
}

async fn register_start(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<RegisterStartRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let ceremonies = require_ceremonies(&state)?;
    let nickname = sanitize_nickname(&req.nickname)?;

    // Exclude credentials the user already has so a single device
    // doesn't enroll twice — webauthn-rs hands the list to the
    // browser which tells the authenticator "skip these."
    let existing = state
        .passkey_repo
        .list_live(actor.user_id)
        .await
        .map_err(ApiError::from)?;
    let excluded: Vec<CredentialID> = existing
        .iter()
        .filter_map(|p| decode_credential_id(&p.credential_id))
        .collect();

    let user_email = lookup_user_email(&state, actor.user_id).await?;

    let (ceremony_id, challenge) = ceremonies
        .start_registration(
            actor.user_id,
            &user_email,
            // The display name that the authenticator shows to the
            // user. Keep it plain (email) until the users table grows
            // a separate display_name column we want to surface here.
            &user_email,
            excluded,
        )
        .await
        .map_err(webauthn_to_api)?;

    // Stash the nickname under the ceremony id so register/finish
    // doesn't require the browser to re-supply it. The
    // `pending_nicknames` map lives in AppState (see main.rs) — it's
    // a simple Mutex<HashMap<Uuid, String>> bounded by the ceremony
    // TTL.
    state
        .passkey_pending_nicknames
        .lock()
        .expect("nickname store poisoned")
        .insert(ceremony_id, nickname);

    Ok(Json(RegisterStartResponse {
        ceremony_id,
        challenge,
    }))
}

#[derive(Deserialize)]
struct RegisterFinishRequest {
    ceremony_id: Uuid,
    /// The browser-supplied attestation. Opaque to us — we hand it
    /// to webauthn-rs.
    register: RegisterPublicKeyCredential,
}

#[derive(Serialize)]
struct PasskeyView {
    id: Uuid,
    nickname: String,
    registered_at: OffsetDateTime,
    last_used_at: Option<OffsetDateTime>,
}

async fn register_finish(
    State(state): State<AppState>,
    actor: Actor,
    Json(req): Json<RegisterFinishRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let ceremonies = require_ceremonies(&state)?;
    let nickname = state
        .passkey_pending_nicknames
        .lock()
        .expect("nickname store poisoned")
        .remove(&req.ceremony_id)
        .unwrap_or_else(|| "Passkey".to_string());

    let CompletedRegistration {
        credential_id,
        credential_blob,
    } = ceremonies
        .finish_registration(req.ceremony_id, &req.register)
        .await
        .map_err(webauthn_to_api)?;

    let row = state
        .passkey_repo
        .insert(actor.user_id, &credential_id, &credential_blob, &nickname)
        .await
        .map_err(ApiError::from)?;

    Ok((
        StatusCode::CREATED,
        Json(PasskeyView {
            id: row.id,
            nickname: row.nickname,
            registered_at: row.registered_at,
            last_used_at: row.last_used_at,
        }),
    ))
}

// --- assertion -------------------------------------------------------------

#[derive(Serialize)]
struct AssertStartResponse {
    ceremony_id: Uuid,
    challenge: RequestChallengeResponse,
}

async fn assert_start(
    State(state): State<AppState>,
    user: SessionUser,
) -> Result<Json<AssertStartResponse>, ApiError> {
    let ceremonies = require_ceremonies(&state)?;

    let live = state
        .passkey_repo
        .list_live(user.user_id)
        .await
        .map_err(ApiError::from)?;
    if live.is_empty() {
        return Err(ApiError::from(PlatformError::Conflict(
            "no registered passkeys; visit /app/settings/security to enroll".into(),
        )));
    }
    let passkeys: Result<Vec<_>, _> = live
        .iter()
        .map(|p| kubinate_identity::webauthn::deserialize_passkey(&p.credential))
        .collect();
    let passkeys = passkeys.map_err(webauthn_to_api)?;

    let (ceremony_id, challenge) = ceremonies
        .start_assertion(user.user_id, &passkeys)
        .await
        .map_err(webauthn_to_api)?;

    Ok(Json(AssertStartResponse {
        ceremony_id,
        challenge,
    }))
}

#[derive(Deserialize)]
struct AssertFinishRequest {
    ceremony_id: Uuid,
    /// The browser-supplied assertion. Opaque to us.
    credential: PublicKeyCredential,
}

#[derive(Serialize)]
struct AssertFinishResponse {
    /// `true` once the partial session has been promoted; the
    /// browser refreshes the page and re-fetches `/v1/me`.
    mfa_satisfied: bool,
}

async fn assert_finish(
    State(state): State<AppState>,
    user: SessionUser,
    Json(req): Json<AssertFinishRequest>,
) -> Result<Json<AssertFinishResponse>, ApiError> {
    let ceremonies = require_ceremonies(&state)?;

    // Look up the passkey row that the assertion's credential id
    // points at — webauthn-rs expects us to pass the matching
    // `Passkey` value into finish_assertion.
    let assertion_cred_id = base64_url_encode(req.credential.id.as_ref());
    let row = state
        .passkey_repo
        .find_by_credential_id(&assertion_cred_id)
        .await
        .map_err(ApiError::from)?
        .ok_or_else(|| {
            ApiError::from(PlatformError::Forbidden(
                "no live passkey matches this assertion".into(),
            ))
        })?;
    if row.user_id != user.user_id {
        return Err(ApiError::from(PlatformError::Forbidden(
            "passkey does not belong to this user".into(),
        )));
    }

    let mut passkey =
        kubinate_identity::webauthn::deserialize_passkey(&row.credential).map_err(webauthn_to_api)?;

    let CompletedAssertion {
        passkey_row_id: _,
        new_sign_counter,
        updated_credential_blob,
    } = ceremonies
        .finish_assertion(req.ceremony_id, row.id, &mut passkey, &req.credential)
        .await
        .map_err(webauthn_to_api)?;

    // Persist the (possibly mutated) blob alongside the counter.
    // Skipping the blob would let webauthn-rs's internal state
    // (counter, backup-eligibility flags, attestation cache)
    // drift from the on-disk row, defeating cloned-authenticator
    // detection on the next assertion.
    state
        .passkey_repo
        .record_use(row.id, new_sign_counter, &updated_credential_blob)
        .await
        .map_err(ApiError::from)?;

    // Promote the *specific* session that initiated this
    // assertion, not the user's most-recent session — a
    // multi-device user with one partial + one full session was
    // previously promoting the wrong one. `Uuid::nil()` in
    // dev-header mode → skip (no real session row exists).
    if !user.session_id.is_nil() {
        session::mark_mfa_satisfied(&state.db, user.session_id)
            .await
            .map_err(ApiError::from)?;
    }

    Ok(Json(AssertFinishResponse { mfa_satisfied: true }))
}

// --- inventory -------------------------------------------------------------

async fn list(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<Json<Vec<PasskeyView>>, ApiError> {
    let rows = state
        .passkey_repo
        .list_live(actor.user_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(
        rows.into_iter()
            .map(|p| PasskeyView {
                id: p.id,
                nickname: p.nickname,
                registered_at: p.registered_at,
                last_used_at: p.last_used_at,
            })
            .collect(),
    ))
}

async fn revoke(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    state
        .passkey_repo
        .revoke(actor.user_id, id)
        .await
        .map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}

// --- helpers ---------------------------------------------------------------

fn require_ceremonies(state: &AppState) -> Result<Arc<Ceremonies>, ApiError> {
    state.ceremonies.clone().ok_or_else(|| {
        ApiError::from(PlatformError::Internal(anyhow::anyhow!(
            "WebAuthn is not configured on this deployment (KUBINATE_WEBAUTHN_RP_ID + KUBINATE_WEBAUTHN_RP_ORIGIN)"
        )))
    })
}

fn sanitize_nickname(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ApiError::from(PlatformError::Invalid(
            "nickname must not be empty".into(),
        )));
    }
    if trimmed.chars().count() > 64 {
        return Err(ApiError::from(PlatformError::Invalid(
            "nickname must be 64 characters or fewer".into(),
        )));
    }
    Ok(trimmed.to_string())
}

fn webauthn_to_api(err: WebauthnError) -> ApiError {
    use WebauthnError::{CeremonyMissing, Config, Rejected, Storage};
    match err {
        // 410 Gone is closer than 404 — the ceremony existed at some
        // point but is now consumed/expired. PlatformError doesn't
        // have a Gone variant; map to NotFound which axum's Problem
        // Details mapper renders as 404. Acceptable for now; a
        // dedicated Problem Details code is a follow-up.
        CeremonyMissing => ApiError::from(PlatformError::NotFound(
            "webauthn ceremony not found or expired".into(),
        )),
        Rejected(msg) => ApiError::from(PlatformError::Forbidden(msg)),
        Config(msg) => ApiError::from(PlatformError::Internal(anyhow::anyhow!(msg))),
        Storage(p) => ApiError::from(p),
    }
}

fn decode_credential_id(s: &str) -> Option<CredentialID> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.decode(s).ok().map(CredentialID::from)
}

fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}

async fn lookup_user_email(state: &AppState, user_id: Uuid) -> Result<String, ApiError> {
    let (email,): (String,) = sqlx::query_as("SELECT email FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .map_err(PlatformError::from)?;
    Ok(email)
}

// --- recovery codes --------------------------------------------------------

#[derive(Serialize)]
struct RecoveryRegenerateResponse {
    /// Plaintext codes. Returned **once** in the response — the
    /// server never has them again. The frontend must show them to
    /// the user with a "save these now" treatment.
    codes: Vec<String>,
    /// Total in the new batch. Equals `codes.len()`; surfaced
    /// separately so the security settings page can render
    /// "10 codes remaining" without recounting.
    total: u32,
}

async fn recovery_regenerate(
    State(state): State<AppState>,
    actor: Actor,
) -> Result<Json<RecoveryRegenerateResponse>, ApiError> {
    // Generate fresh batch — entropy from the OS RNG. Scoped tightly
    // because `ThreadRng` is `!Send`; holding it across the .await
    // below would bubble up as a Handler trait error.
    let plaintext_codes = {
        let mut rng = rand::thread_rng();
        rc::generate_batch(&mut rng)
    };
    let hashes: Vec<[u8; 32]> = plaintext_codes.iter().map(|c| rc::hash(c)).collect();

    // rotate_batch drops every prior row in the same transaction,
    // so old codes — used or not — are immediately invalid.
    state
        .recovery_codes_repo
        .rotate_batch(actor.user_id, &hashes)
        .await
        .map_err(ApiError::from)?;

    #[allow(clippy::cast_possible_truncation)]
    let total = plaintext_codes.len() as u32;
    Ok(Json(RecoveryRegenerateResponse {
        codes: plaintext_codes,
        total,
    }))
}

#[derive(Deserialize)]
struct RecoveryRedeemRequest {
    /// User-supplied code, in any case + with or without the
    /// hyphen. Hashing canonicalises before comparison.
    code: String,
}

#[derive(Serialize)]
struct RecoveryRedeemResponse {
    /// `true` once the partial session has been promoted.
    mfa_satisfied: bool,
    /// How many recovery codes the user has left after redeeming
    /// this one. The UI uses this to surface "regenerate now"
    /// guidance when the count is low.
    remaining: u32,
}

async fn recovery_redeem(
    State(state): State<AppState>,
    user: SessionUser,
    Json(req): Json<RecoveryRedeemRequest>,
) -> Result<Json<RecoveryRedeemResponse>, ApiError> {
    let hash = rc::hash(&req.code);
    let consumed = state
        .recovery_codes_repo
        .try_consume(user.user_id, &hash)
        .await
        .map_err(ApiError::from)?;
    if !consumed {
        // Same response shape as a wrong WebAuthn assertion —
        // attacker can't distinguish "no such code" from "wrong
        // code" from a 403 alone. Audit-log enrichment is a
        // follow-up so a brute-force attempt still surfaces.
        return Err(ApiError::from(PlatformError::Forbidden(
            "recovery code is invalid or already used".into(),
        )));
    }

    // Promote the specific session, not the user's most-recent.
    // Same reasoning as `assert_finish`.
    if !user.session_id.is_nil() {
        session::mark_mfa_satisfied(&state.db, user.session_id)
            .await
            .map_err(ApiError::from)?;
    }

    let remaining = state
        .recovery_codes_repo
        .count_live(user.user_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(RecoveryRedeemResponse {
        mfa_satisfied: true,
        remaining,
    }))
}

