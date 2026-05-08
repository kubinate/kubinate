//! Stripe API client + webhook signature verification.
//!
//! Sprint 2 ticket 08 ships only what the billing-scaffolding ACs
//! need: create a Checkout Session, and verify the signature on a
//! webhook delivery. Customer portal sessions, invoice queries, and
//! subscription manipulation land in later sprints.
//!
//! The signature verifier is **not** an HTTP client — it's a pure
//! function over `(payload_bytes, header_value, secret, now)` so the
//! API handler can call it before persisting anything.

use std::time::Duration;

use async_trait::async_trait;
use hmac_sha256::HMAC;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use thiserror::Error;

/// Default tolerance for the `Stripe-Signature` header timestamp.
/// Stripe's official libraries use 5 minutes.
pub const STRIPE_TIMESTAMP_TOLERANCE: Duration = Duration::from_secs(5 * 60);

/// Errors raised by the Stripe client.
#[derive(Debug, Error)]
pub enum StripeError {
    /// Low-level HTTP / transport failure.
    #[error(transparent)]
    Transport(#[from] reqwest::Error),
    /// Stripe returned a non-2xx; body truncated to 512 chars.
    #[error("stripe api status {status}: {body}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// Truncated response body.
        body: String,
    },
}

/// Errors raised by [`verify_signature`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SignatureError {
    /// `Stripe-Signature` header missing or malformed.
    #[error("malformed signature header")]
    Malformed,
    /// Timestamp inside the header is outside the tolerance window.
    #[error("timestamp outside tolerance")]
    Timestamp,
    /// Computed HMAC didn't match any of the supplied `v1` signatures.
    #[error("signature did not match")]
    Mismatch,
}

/// Parameters for [`StripeClient::create_checkout_session`].
#[derive(Debug, Clone)]
pub struct CheckoutSessionParams {
    /// Stripe price id (`price_…`). Owners pick a plan in the UI; the
    /// API translates plan slug → price id from a static map (Sprint 3).
    pub price_id: String,
    /// Where the user is sent after a successful subscribe.
    pub success_url: String,
    /// Where the user is sent if they bail.
    pub cancel_url: String,
    /// Existing Stripe customer id, if we already have one for this org.
    pub customer_id: Option<String>,
    /// New customer email — used by Stripe to create the customer when
    /// `customer_id` is absent.
    pub customer_email: Option<String>,
    /// Free-form metadata. We always include the kubinate `organization_id`
    /// so webhook events can be routed back to the right tenant.
    pub metadata: std::collections::BTreeMap<String, String>,
}

/// Subset of the Stripe Checkout Session response we care about.
#[derive(Debug, Clone, Deserialize)]
pub struct CheckoutSession {
    /// Hosted-checkout URL for the customer to complete payment.
    pub url: String,
    /// `cus_…` — assigned by Stripe if a new customer was created.
    /// We persist this back onto `organizations.stripe_customer_id`.
    pub customer: Option<String>,
}

/// Trait surface the API uses. Production wraps `api.stripe.com`; the
/// signature-verification unit tests don't need this trait at all.
#[async_trait]
pub trait StripeClient: Send + Sync {
    /// Create a hosted Checkout Session.
    async fn create_checkout_session(
        &self,
        params: CheckoutSessionParams,
    ) -> Result<CheckoutSession, StripeError>;
}

/// HTTP client implementation.
pub struct HttpStripeClient {
    secret_key: SecretString,
    http: reqwest::Client,
    base_url: String,
}

impl HttpStripeClient {
    /// Build a client pinned to `https://api.stripe.com/v1`.
    #[must_use]
    pub fn new(secret_key: SecretString) -> Self {
        Self::with_base_url(secret_key, "https://api.stripe.com/v1".into())
    }

    /// Build a client against a custom base URL — used by tests that
    /// point a wiremock at us.
    #[must_use]
    pub fn with_base_url(secret_key: SecretString, base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(concat!("kubinate/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("reqwest build");
        Self {
            secret_key,
            http,
            base_url,
        }
    }
}

#[async_trait]
impl StripeClient for HttpStripeClient {
    async fn create_checkout_session(
        &self,
        params: CheckoutSessionParams,
    ) -> Result<CheckoutSession, StripeError> {
        // Stripe's API is form-encoded, with `[]` notation for arrays /
        // nested objects. We build the body explicitly to avoid the
        // weight of a full Stripe SDK.
        let mut form: Vec<(String, String)> = Vec::new();
        form.push(("mode".into(), "subscription".into()));
        form.push(("line_items[0][price]".into(), params.price_id));
        form.push(("line_items[0][quantity]".into(), "1".into()));
        form.push(("success_url".into(), params.success_url));
        form.push(("cancel_url".into(), params.cancel_url));
        if let Some(c) = params.customer_id {
            form.push(("customer".into(), c));
        }
        if let Some(e) = params.customer_email {
            form.push(("customer_email".into(), e));
        }
        for (k, v) in params.metadata {
            form.push((format!("metadata[{k}]"), v));
        }

        let url = format!("{}/checkout/sessions", self.base_url);
        let response = self
            .http
            .post(&url)
            .basic_auth(self.secret_key.expose_secret(), Some(""))
            .form(&form)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(StripeError::Status {
                status: status.as_u16(),
                body: body.chars().take(512).collect(),
            });
        }
        Ok(response.json::<CheckoutSession>().await?)
    }
}

/// Verify the `Stripe-Signature` header against the raw request body.
///
/// Implements the algorithm from Stripe's official docs:
///   * parse `t=<timestamp>,v1=<sig>[,v1=<sig>...]`
///   * reject if `now - t > tolerance`
///   * compute `HMAC_SHA256(secret, "<t>.<payload>")`
///   * accept if any `v1` candidate matches in constant time
///
/// The `now_unix_secs` argument is injectable for tests; production
/// passes `SystemTime::now()`.
///
/// # Errors
/// One of [`SignatureError`]'s variants.
pub fn verify_signature(
    payload: &[u8],
    signature_header: &str,
    secret: &SecretString,
    now_unix_secs: i64,
    tolerance: Duration,
) -> Result<(), SignatureError> {
    let mut timestamp: Option<i64> = None;
    let mut signatures: Vec<String> = Vec::new();
    for part in signature_header.split(',') {
        let mut kv = part.splitn(2, '=');
        let k = kv.next().unwrap_or("").trim();
        let v = kv.next().ok_or(SignatureError::Malformed)?.trim();
        match k {
            "t" => timestamp = v.parse().ok(),
            "v1" => signatures.push(v.to_string()),
            _ => {} // forward-compat: silently ignore newer scheme versions
        }
    }
    let timestamp = timestamp.ok_or(SignatureError::Malformed)?;
    if signatures.is_empty() {
        return Err(SignatureError::Malformed);
    }

    let drift = (now_unix_secs - timestamp).abs();
    if drift > i64::try_from(tolerance.as_secs()).unwrap_or(i64::MAX) {
        return Err(SignatureError::Timestamp);
    }

    let mut signed_payload = Vec::with_capacity(20 + payload.len());
    signed_payload.extend_from_slice(timestamp.to_string().as_bytes());
    signed_payload.push(b'.');
    signed_payload.extend_from_slice(payload);

    let mac = HMAC::mac(&signed_payload, secret.expose_secret().as_bytes());
    let expected_hex = hex_lower(&mac);

    if signatures
        .iter()
        .any(|s| constant_time_eq_ignore_ascii_case(s.as_bytes(), expected_hex.as_bytes()))
    {
        Ok(())
    } else {
        Err(SignatureError::Mismatch)
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn constant_time_eq_ignore_ascii_case(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x.eq_ignore_ascii_case(y) as u8 ^ 1;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(payload: &[u8], secret: &str, ts: i64) -> String {
        let mut signed = Vec::new();
        signed.extend_from_slice(ts.to_string().as_bytes());
        signed.push(b'.');
        signed.extend_from_slice(payload);
        let mac = HMAC::mac(&signed, secret.as_bytes());
        format!("t={ts},v1={}", hex_lower(&mac))
    }

    #[test]
    fn known_good_signature_verifies() {
        let body = br#"{"id":"evt_1","type":"checkout.session.completed"}"#;
        let secret = SecretString::from("whsec_test");
        let ts = 1_700_000_000;
        let header = sign(body, "whsec_test", ts);
        verify_signature(body, &header, &secret, ts, STRIPE_TIMESTAMP_TOLERANCE).unwrap();
    }

    #[test]
    fn tampered_payload_is_rejected() {
        let body = br#"{"id":"evt_1","type":"checkout.session.completed"}"#;
        let secret = SecretString::from("whsec_test");
        let ts = 1_700_000_000;
        let header = sign(body, "whsec_test", ts);
        let tampered = br#"{"id":"evt_1","type":"checkout.session.expired"}"#;
        let err = verify_signature(tampered, &header, &secret, ts, STRIPE_TIMESTAMP_TOLERANCE)
            .unwrap_err();
        assert_eq!(err, SignatureError::Mismatch);
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let body = b"hello";
        let ts = 1_700_000_000;
        let header = sign(body, "whsec_one", ts);
        let other = SecretString::from("whsec_two");
        let err =
            verify_signature(body, &header, &other, ts, STRIPE_TIMESTAMP_TOLERANCE).unwrap_err();
        assert_eq!(err, SignatureError::Mismatch);
    }

    #[test]
    fn timestamp_outside_tolerance_is_rejected() {
        let body = b"hello";
        let secret = SecretString::from("whsec_test");
        let ts = 1_700_000_000;
        let header = sign(body, "whsec_test", ts);
        // Eight minutes after the signed timestamp -> outside the
        // 5-minute window.
        let now = ts + 8 * 60;
        let err =
            verify_signature(body, &header, &secret, now, STRIPE_TIMESTAMP_TOLERANCE).unwrap_err();
        assert_eq!(err, SignatureError::Timestamp);
    }

    #[test]
    fn malformed_header_rejected() {
        let secret = SecretString::from("whsec_test");
        let err = verify_signature(
            b"x",
            "no-equals-here",
            &secret,
            0,
            STRIPE_TIMESTAMP_TOLERANCE,
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::Malformed);
    }

    #[test]
    fn missing_v1_rejected() {
        let secret = SecretString::from("whsec_test");
        let err = verify_signature(
            b"x",
            "t=1700000000,v0=deadbeef",
            &secret,
            1_700_000_000,
            STRIPE_TIMESTAMP_TOLERANCE,
        )
        .unwrap_err();
        assert_eq!(err, SignatureError::Malformed);
    }
}
