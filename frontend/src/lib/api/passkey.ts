/**
 * Sprint 4 ticket 05 — typed wrappers for `/v1/auth/passkey/*` and
 * `/v1/auth/recovery-codes/*`.
 *
 * The page (`/app/settings/security`) calls into here rather than
 * hand-rolling fetch — keeps the wire-format expectations in one
 * file and lets future contracts changes (Problem Details `code`
 * semantics, response-shape additions) land in one place.
 *
 * WebAuthn-specific note: the `*Start` calls return the raw
 * `webauthn-rs` challenge JSON as `unknown`. Converting that JSON
 * into the browser's `PublicKeyCredentialCreationOptions` /
 * `PublicKeyCredentialRequestOptions` (Base64URL → ArrayBuffer)
 * happens inside [`webauthnJsonToCreate`] /
 * [`webauthnJsonToGet`] below. The reverse — cleaning the
 * authenticator's response into JSON-friendly Base64URL — is
 * [`credentialToJson`].
 */

import { ApiError, parseProblem } from './_problem';
import {
  assertFinishResponseSchema,
  assertStartResponseSchema,
  passkeyViewSchema,
  recoveryRedeemResponseSchema,
  recoveryRegenerateResponseSchema,
  registerStartResponseSchema,
  type AssertFinishResponse,
  type AssertStartResponse,
  type PasskeyView,
  type RecoveryRedeemResponse,
  type RecoveryRegenerateResponse,
  type RegisterStartResponse
} from './schemas';
import { z } from 'zod';

// --- registration ----------------------------------------------------------

/**
 * Open a new passkey registration. The browser uses the returned
 * challenge to call `navigator.credentials.create()`.
 */
export async function startRegister(
  nickname: string,
  fetchImpl: typeof fetch = fetch
): Promise<RegisterStartResponse> {
  const r = await fetchImpl('/api/v1/auth/passkey/register/start', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ nickname })
  });
  return parseOrThrow(r, registerStartResponseSchema);
}

/**
 * Complete the registration. `attestation` is the JSON-friendly
 * shape returned by [`credentialToJson`].
 */
export async function finishRegister(
  ceremonyId: string,
  attestation: unknown,
  fetchImpl: typeof fetch = fetch
): Promise<PasskeyView> {
  const r = await fetchImpl('/api/v1/auth/passkey/register/finish', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ ceremony_id: ceremonyId, register: attestation })
  });
  return parseOrThrow(r, passkeyViewSchema);
}

// --- assertion (partial-MFA-session promotion) ----------------------------

export async function startAssert(fetchImpl: typeof fetch = fetch): Promise<AssertStartResponse> {
  const r = await fetchImpl('/api/v1/auth/passkey/assert/start', {
    method: 'POST'
  });
  return parseOrThrow(r, assertStartResponseSchema);
}

export async function finishAssert(
  ceremonyId: string,
  credential: unknown,
  fetchImpl: typeof fetch = fetch
): Promise<AssertFinishResponse> {
  const r = await fetchImpl('/api/v1/auth/passkey/assert/finish', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ ceremony_id: ceremonyId, credential })
  });
  return parseOrThrow(r, assertFinishResponseSchema);
}

// --- inventory --------------------------------------------------------------

export async function listPasskeys(fetchImpl: typeof fetch = fetch): Promise<PasskeyView[]> {
  const r = await fetchImpl('/api/v1/auth/passkey/list');
  return parseOrThrow(r, z.array(passkeyViewSchema));
}

export async function revokePasskey(id: string, fetchImpl: typeof fetch = fetch): Promise<void> {
  const r = await fetchImpl(`/api/v1/auth/passkey/${id}`, { method: 'DELETE' });
  if (!r.ok) {
    throw await parseProblem(r);
  }
}

// --- recovery codes ---------------------------------------------------------

export async function regenerateRecoveryCodes(
  fetchImpl: typeof fetch = fetch
): Promise<RecoveryRegenerateResponse> {
  const r = await fetchImpl('/api/v1/auth/recovery-codes/regenerate', {
    method: 'POST'
  });
  return parseOrThrow(r, recoveryRegenerateResponseSchema);
}

export async function redeemRecoveryCode(
  code: string,
  fetchImpl: typeof fetch = fetch
): Promise<RecoveryRedeemResponse> {
  const r = await fetchImpl('/api/v1/auth/recovery-codes/redeem', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ code })
  });
  return parseOrThrow(r, recoveryRedeemResponseSchema);
}

// --- WebAuthn JSON ↔ browser-API helpers -----------------------------------

/**
 * Convert a Base64URL string the API returned into the
 * `BufferSource` the browser's WebAuthn API expects.
 */
function base64UrlToBuffer(input: string): Uint8Array {
  const padding = '='.repeat((4 - (input.length % 4)) % 4);
  const base64 = (input + padding).replace(/-/g, '+').replace(/_/g, '/');
  const raw = atob(base64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i += 1) out[i] = raw.charCodeAt(i);
  return out;
}

/**
 * Convert a `BufferSource` back into the Base64URL string shape the
 * Rust API expects on the wire.
 */
function bufferToBase64Url(input: ArrayBuffer): string {
  const bytes = new Uint8Array(input);
  let binary = '';
  for (let i = 0; i < bytes.byteLength; i += 1) binary += String.fromCharCode(bytes[i]);
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/**
 * Take a `webauthn-rs::CreationChallengeResponse` JSON blob and
 * convert the Base64URL fields into ArrayBuffers so the browser's
 * `navigator.credentials.create({ publicKey })` accepts it.
 *
 * The shape of the JSON is fixed by the WebAuthn Level 3 spec; we
 * only touch the fields whose values are encoded as Base64URL in
 * the JSON form.
 */
export function webauthnJsonToCreate(json: unknown): CredentialCreationOptions {
  const opts = json as {
    publicKey: {
      challenge: string;
      user: { id: string; name: string; displayName: string };
      excludeCredentials?: Array<{ id: string; type: string }>;
    } & Record<string, unknown>;
  };
  const pk = opts.publicKey;
  // Cast goes through `unknown` because the WebAuthn JSON shape
  // overlaps with `PublicKeyCredentialCreationOptions` only after
  // we substitute the Base64URL strings with ArrayBuffers; TS sees
  // a Record<string, unknown> + a few overrides and can't statically
  // confirm every required field (`rp`, `pubKeyCredParams`) is
  // present — but `webauthn-rs` always emits them.
  return {
    publicKey: {
      ...pk,
      challenge: base64UrlToBuffer(pk.challenge),
      user: { ...pk.user, id: base64UrlToBuffer(pk.user.id) },
      excludeCredentials: (pk.excludeCredentials ?? []).map((c) => ({
        ...c,
        id: base64UrlToBuffer(c.id),
        type: c.type as PublicKeyCredentialType
      }))
    } as unknown as PublicKeyCredentialCreationOptions
  };
}

/**
 * Same shape as [`webauthnJsonToCreate`], for the assertion path.
 */
export function webauthnJsonToGet(json: unknown): CredentialRequestOptions {
  const opts = json as {
    publicKey: {
      challenge: string;
      allowCredentials?: Array<{ id: string; type: string }>;
    } & Record<string, unknown>;
  };
  const pk = opts.publicKey;
  return {
    publicKey: {
      ...pk,
      challenge: base64UrlToBuffer(pk.challenge),
      allowCredentials: (pk.allowCredentials ?? []).map((c) => ({
        ...c,
        id: base64UrlToBuffer(c.id),
        type: c.type as PublicKeyCredentialType
      }))
    } as unknown as PublicKeyCredentialRequestOptions
  };
}

/**
 * Convert a `PublicKeyCredential` returned by the browser into the
 * JSON shape `webauthn-rs` expects on the wire.
 */
export function credentialToJson(credential: PublicKeyCredential): unknown {
  const response = credential.response;
  const base = {
    id: credential.id,
    rawId: bufferToBase64Url(credential.rawId),
    type: credential.type,
    clientExtensionResults: credential.getClientExtensionResults()
  };
  if ('attestationObject' in response) {
    // Registration: AuthenticatorAttestationResponse.
    const r = response as AuthenticatorAttestationResponse;
    return {
      ...base,
      response: {
        clientDataJSON: bufferToBase64Url(r.clientDataJSON),
        attestationObject: bufferToBase64Url(r.attestationObject)
      }
    };
  }
  // Assertion: AuthenticatorAssertionResponse.
  const r = response as AuthenticatorAssertionResponse;
  return {
    ...base,
    response: {
      clientDataJSON: bufferToBase64Url(r.clientDataJSON),
      authenticatorData: bufferToBase64Url(r.authenticatorData),
      signature: bufferToBase64Url(r.signature),
      userHandle: r.userHandle ? bufferToBase64Url(r.userHandle) : null
    }
  };
}

// --- internal helpers -------------------------------------------------------

async function parseOrThrow<T>(
  response: Response,
  schema: { parse: (u: unknown) => T }
): Promise<T> {
  const text = await response.text();
  if (!response.ok) throw await parseProblem(response, text);
  return schema.parse(text ? JSON.parse(text) : {});
}

export { ApiError };
