<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import {
    ApiError,
    credentialToJson,
    finishAssert,
    finishRegister,
    listPasskeys,
    redeemRecoveryCode,
    regenerateRecoveryCodes,
    revokePasskey,
    startAssert,
    startRegister,
    webauthnJsonToCreate,
    webauthnJsonToGet
  } from '$lib/api/passkey';
  import type { PasskeyView } from '$lib/api/schemas';

  // Sprint 4 ticket 05 — settings/security surfaces three things:
  //   1. The user's existing passkeys (list + register + revoke).
  //   2. Recovery codes (regenerate-and-show-once).
  //   3. The MFA assertion prompt — only when the API redirected
  //      here with `?mfa=required` because a privileged session is
  //      sitting in the partial-MFA-session state.
  //
  // Rendering all three on one page keeps the WebAuthn/recovery
  // surface cohesive and matches the AC list in the ticket — there
  // is no separate "MFA challenge" route.

  // --- list state ------------------------------------------------------------

  let passkeys = $state<PasskeyView[] | null>(null);
  let loadError = $state<string | null>(null);

  async function refreshPasskeys() {
    try {
      passkeys = await listPasskeys();
      loadError = null;
    } catch (err) {
      loadError = describe(err);
    }
  }

  function describe(err: unknown): string {
    if (err instanceof ApiError) return `${err.title}: ${err.detail}`;
    if (err instanceof Error) return err.message;
    return 'Unknown error';
  }

  // --- register flow ---------------------------------------------------------

  let registerModalOpen = $state(false);
  let registerNickname = $state('');
  let registerInFlight = $state(false);
  let registerError = $state<string | null>(null);

  function openRegisterModal() {
    registerNickname = '';
    registerError = null;
    registerModalOpen = true;
  }

  function closeRegisterModal() {
    if (registerInFlight) return;
    registerModalOpen = false;
    registerError = null;
  }

  async function confirmRegister() {
    const nickname = registerNickname.trim();
    if (nickname.length === 0) {
      registerError = 'Nickname is required';
      return;
    }
    if (nickname.length > 64) {
      registerError = 'Nickname must be 64 characters or fewer';
      return;
    }
    registerInFlight = true;
    registerError = null;
    try {
      const start = await startRegister(nickname);
      const options = webauthnJsonToCreate(start.challenge);
      const credential = await navigator.credentials.create(options);
      if (!credential) {
        registerError = 'authenticator returned no credential';
        return;
      }
      const json = credentialToJson(credential as PublicKeyCredential);
      await finishRegister(start.ceremony_id, json);
      registerModalOpen = false;
      await refreshPasskeys();
    } catch (err) {
      registerError = describe(err);
    } finally {
      registerInFlight = false;
    }
  }

  // --- revoke ---------------------------------------------------------------

  let revokingId = $state<string | null>(null);
  let revokeError = $state<string | null>(null);

  async function revoke(id: string, nickname: string) {
    if (!window.confirm(`Revoke passkey "${nickname}"? This cannot be undone.`)) return;
    revokingId = id;
    revokeError = null;
    try {
      await revokePasskey(id);
      await refreshPasskeys();
    } catch (err) {
      revokeError = describe(err);
    } finally {
      revokingId = null;
    }
  }

  // --- recovery code regenerate ---------------------------------------------

  let recoveryModalOpen = $state(false);
  let recoveryCodes = $state<string[] | null>(null);
  let recoveryCopied = $state(false);
  let recoveryInFlight = $state(false);
  let recoveryError = $state<string | null>(null);

  async function regenerate() {
    recoveryInFlight = true;
    recoveryError = null;
    recoveryCopied = false;
    try {
      const result = await regenerateRecoveryCodes();
      recoveryCodes = result.codes;
      recoveryModalOpen = true;
    } catch (err) {
      recoveryError = describe(err);
    } finally {
      recoveryInFlight = false;
    }
  }

  async function copyCodes() {
    if (!recoveryCodes) return;
    try {
      await navigator.clipboard.writeText(recoveryCodes.join('\n'));
      recoveryCopied = true;
    } catch (err) {
      recoveryError = describe(err);
    }
  }

  function tryCloseRecoveryModal() {
    if (
      !recoveryCopied &&
      !window.confirm("You haven't copied your recovery codes yet — are you sure?")
    ) {
      return;
    }
    recoveryModalOpen = false;
    // Wipe in-memory copy after the user dismisses; the server
    // can't re-emit them, so keeping them around in the page state
    // would be a needless XSS-residue surface.
    recoveryCodes = null;
    recoveryCopied = false;
  }

  // --- MFA assertion (only when ?mfa=required) ------------------------------

  const mfaRequired = $derived(page.url.searchParams.get('mfa') === 'required');

  let assertInFlight = $state(false);
  let assertError = $state<string | null>(null);

  async function assert() {
    assertInFlight = true;
    assertError = null;
    try {
      const start = await startAssert();
      const options = webauthnJsonToGet(start.challenge);
      const credential = await navigator.credentials.get(options);
      if (!credential) {
        assertError = 'authenticator returned no credential';
        return;
      }
      const json = credentialToJson(credential as PublicKeyCredential);
      await finishAssert(start.ceremony_id, json);
      await goto('/app/clusters');
    } catch (err) {
      assertError = describe(err);
    } finally {
      assertInFlight = false;
    }
  }

  // --- recovery redeem -------------------------------------------------------

  let redeemModalOpen = $state(false);
  let redeemCode = $state('');
  let redeemInFlight = $state(false);
  let redeemError = $state<string | null>(null);

  function openRedeemModal() {
    redeemCode = '';
    redeemError = null;
    redeemModalOpen = true;
  }

  function closeRedeemModal() {
    if (redeemInFlight) return;
    redeemModalOpen = false;
  }

  async function confirmRedeem() {
    const code = redeemCode.trim();
    if (code.length === 0) {
      redeemError = 'Enter a recovery code';
      return;
    }
    redeemInFlight = true;
    redeemError = null;
    try {
      await redeemRecoveryCode(code);
      redeemModalOpen = false;
      await goto('/app/clusters');
    } catch (err) {
      // 403 from the API maps to "code invalid or already used";
      // the wording matches the AC. Anything else (network, 500)
      // falls through to the generic describe().
      if (err instanceof ApiError && err.status === 403) {
        redeemError = 'code invalid or already used';
      } else {
        redeemError = describe(err);
      }
    } finally {
      redeemInFlight = false;
    }
  }

  // --- formatting -----------------------------------------------------------

  function fmtDate(iso: string): string {
    return new Date(iso).toLocaleDateString();
  }

  function fmtLastUsed(iso: string | null | undefined): string {
    if (!iso) return 'never used';
    return new Date(iso).toLocaleString();
  }

  onMount(refreshPasskeys);
</script>

<svelte:head>
  <title>Security — Kubinate</title>
</svelte:head>

<h1>Security</h1>

{#if mfaRequired}
  <section class="mfa-prompt" aria-live="polite">
    <h2>Verify your identity</h2>
    <p>Your role requires a passkey check before we let you continue.</p>
    <div class="mfa-actions">
      <button type="button" onclick={assert} disabled={assertInFlight} data-testid="assert-button">
        {assertInFlight ? 'Verifying…' : 'Verify with passkey'}
      </button>
      <button
        type="button"
        class="link"
        onclick={openRedeemModal}
        data-testid="recovery-redeem-link"
      >
        Use a recovery code
      </button>
    </div>
    {#if assertError}
      <p class="error" role="alert">{assertError}</p>
    {/if}
  </section>
{/if}

<section class="passkeys">
  <h2>Passkeys</h2>
  <p class="muted">
    Passkeys replace passwords for owner and admin sign-ins. Register one per device.
  </p>

  <div class="section-actions">
    <button type="button" onclick={openRegisterModal} data-testid="register-button">
      Register a passkey
    </button>
  </div>

  {#if loadError}
    <p class="error" role="alert">{loadError}</p>
  {:else if passkeys === null}
    <p class="muted">Loading…</p>
  {:else if passkeys.length === 0}
    <p class="muted">No passkeys yet.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Nickname</th>
          <th>Registered</th>
          <th>Last used</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each passkeys as p (p.id)}
          <tr data-testid={`passkey-row-${p.id}`}>
            <td>{p.nickname}</td>
            <td>{fmtDate(p.registered_at)}</td>
            <td>{fmtLastUsed(p.last_used_at)}</td>
            <td>
              <button
                type="button"
                class="danger"
                onclick={() => revoke(p.id, p.nickname)}
                disabled={revokingId === p.id}
                data-testid={`revoke-${p.id}`}
              >
                {revokingId === p.id ? 'Revoking…' : 'Revoke'}
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}

  {#if revokeError}
    <p class="error" role="alert">{revokeError}</p>
  {/if}
</section>

<section class="recovery">
  <h2>Recovery codes</h2>
  <p class="muted">
    Single-use codes you can redeem if you lose every passkey. Regenerating invalidates any
    previously issued codes.
  </p>
  <div class="section-actions">
    <button
      type="button"
      onclick={regenerate}
      disabled={recoveryInFlight}
      data-testid="regenerate-button"
    >
      {recoveryInFlight ? 'Generating…' : 'Regenerate'}
    </button>
  </div>
  {#if recoveryError && !recoveryModalOpen}
    <p class="error" role="alert">{recoveryError}</p>
  {/if}
</section>

{#if registerModalOpen}
  <div class="modal-backdrop" role="dialog" aria-modal="true">
    <div class="modal">
      <h3>Register a passkey</h3>
      <p class="muted">
        Pick a name that helps you recognise this device later (e.g. "MacBook" or "YubiKey 5C").
      </p>
      <label>
        Nickname
        <input
          type="text"
          bind:value={registerNickname}
          maxlength="64"
          required
          data-testid="register-nickname"
        />
      </label>
      {#if registerError}
        <p class="error" role="alert">{registerError}</p>
      {/if}
      <div class="modal-actions">
        <button type="button" onclick={closeRegisterModal} disabled={registerInFlight}>
          Cancel
        </button>
        <button
          type="button"
          onclick={confirmRegister}
          disabled={registerInFlight}
          data-testid="register-confirm"
        >
          {registerInFlight ? 'Registering…' : 'Register'}
        </button>
      </div>
    </div>
  </div>
{/if}

{#if recoveryModalOpen && recoveryCodes}
  <div class="modal-backdrop" role="dialog" aria-modal="true" data-testid="recovery-modal">
    <div class="modal">
      <h3>Save your recovery codes</h3>
      <p>These codes will not be shown again. Save them in your password manager now.</p>
      <pre data-testid="recovery-codes-list">{recoveryCodes.join('\n')}</pre>
      {#if recoveryError}
        <p class="error" role="alert">{recoveryError}</p>
      {/if}
      <div class="modal-actions">
        <button type="button" onclick={tryCloseRecoveryModal}>Close</button>
        <button type="button" onclick={copyCodes} data-testid="copy-codes">
          {recoveryCopied ? 'Copied' : 'Copy all'}
        </button>
      </div>
    </div>
  </div>
{/if}

{#if redeemModalOpen}
  <div class="modal-backdrop" role="dialog" aria-modal="true">
    <div class="modal">
      <h3>Use a recovery code</h3>
      <p class="muted">Each code works once. After redeeming, regenerate a fresh batch.</p>
      <label>
        Recovery code
        <input
          type="text"
          bind:value={redeemCode}
          placeholder="AB12C-D3E4F"
          autocomplete="one-time-code"
          spellcheck="false"
          data-testid="recovery-code-input"
        />
      </label>
      {#if redeemError}
        <p class="error" role="alert">{redeemError}</p>
      {/if}
      <div class="modal-actions">
        <button type="button" onclick={closeRedeemModal} disabled={redeemInFlight}> Cancel </button>
        <button
          type="button"
          onclick={confirmRedeem}
          disabled={redeemInFlight}
          data-testid="recovery-redeem-confirm"
        >
          {redeemInFlight ? 'Redeeming…' : 'Redeem'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  h1 {
    font-size: 1.8rem;
    margin-bottom: 1rem;
  }
  h2 {
    font-size: 1.1rem;
    margin: 1.5rem 0 0.5rem;
  }
  section {
    max-width: 720px;
    margin-bottom: 2rem;
  }
  .muted {
    color: #555;
  }
  .error {
    color: #b00020;
  }
  .section-actions {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
    margin: 0.5rem 0 0.75rem;
  }
  .section-actions button,
  .mfa-actions button {
    padding: 0.5rem 1rem;
    background: #1a7f37;
    color: #fff;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  .section-actions button[disabled],
  .mfa-actions button[disabled] {
    background: #888;
    cursor: not-allowed;
  }
  .mfa-prompt {
    padding: 1rem 1.25rem;
    background: #fff8e1;
    border: 1px solid #f1d27a;
    border-radius: 6px;
  }
  .mfa-prompt h2 {
    margin-top: 0;
    color: #8a6a00;
  }
  .mfa-actions {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    flex-wrap: wrap;
  }
  .mfa-actions .link {
    background: transparent;
    color: #1a7f37;
    text-decoration: underline;
    padding: 0.5rem 0;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th,
  td {
    text-align: left;
    padding: 0.5rem 0.5rem;
    border-bottom: 1px solid #eee;
  }
  th {
    font-size: 0.85rem;
    color: #555;
    font-weight: 600;
  }
  button.danger {
    padding: 0.4rem 0.8rem;
    background: #b00020;
    color: #fff;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  button.danger[disabled] {
    background: #888;
    cursor: not-allowed;
  }
  pre {
    background: #f6f8fa;
    border: 1px solid #ddd;
    border-radius: 4px;
    padding: 0.75rem;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 0.95rem;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 50;
  }
  .modal {
    background: #fff;
    border-radius: 8px;
    padding: 1.25rem 1.5rem;
    width: min(480px, 90vw);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.2);
  }
  .modal h3 {
    margin: 0 0 0.5rem;
  }
  .modal label {
    display: grid;
    gap: 0.25rem;
    font-weight: 600;
    margin-top: 0.75rem;
  }
  .modal input {
    padding: 0.4rem;
    font: inherit;
    border: 1px solid #bbb;
    border-radius: 4px;
    font-weight: 400;
  }
  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1rem;
  }
  .modal-actions button {
    padding: 0.5rem 1rem;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  .modal-actions button:first-child {
    background: #eee;
    color: #222;
  }
  .modal-actions button:last-child {
    background: #1a7f37;
    color: #fff;
  }
  .modal-actions button[disabled] {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
