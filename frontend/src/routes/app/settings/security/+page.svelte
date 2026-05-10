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
  import { listApiKeys, createApiKey, revokeApiKey, type ApiKeyView } from '$lib/api/api-keys';
  import { Button } from '$lib/components/ui/button';
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent
  } from '$lib/components/ui/card';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';
  import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
    DialogDescription,
    DialogFooter
  } from '$lib/components/ui/dialog';
  import {
    Table,
    TableHeader,
    TableRow,
    TableHead,
    TableBody,
    TableCell
  } from '$lib/components/ui/table';
  import { Plus, RefreshCw, KeyRound, ShieldCheck } from 'lucide-svelte';

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

  // --- API key state ---------------------------------------------------------

  let apiKeys = $state<ApiKeyView[] | null>(null);
  let apiKeyLoadError = $state<string | null>(null);

  let apiKeyCreateOpen = $state(false);
  let apiKeyName = $state('');
  let apiKeyCreateInFlight = $state(false);
  let apiKeyCreateError = $state<string | null>(null);
  let newKeyToken = $state<string | null>(null);
  let newKeyTokenCopied = $state(false);

  let revokingKeyId = $state<string | null>(null);
  let apiKeyRevokeError = $state<string | null>(null);

  async function refreshApiKeys() {
    try {
      apiKeys = await listApiKeys();
      apiKeyLoadError = null;
    } catch (err) {
      apiKeyLoadError = describe(err);
    }
  }

  function openApiKeyCreateModal() {
    apiKeyName = '';
    apiKeyCreateError = null;
    newKeyToken = null;
    newKeyTokenCopied = false;
    apiKeyCreateOpen = true;
  }

  function closeApiKeyCreateModal() {
    if (apiKeyCreateInFlight) return;
    apiKeyCreateOpen = false;
    newKeyToken = null;
    newKeyTokenCopied = false;
  }

  async function confirmCreateApiKey() {
    const name = apiKeyName.trim();
    if (name.length === 0) {
      apiKeyCreateError = 'Name is required';
      return;
    }
    apiKeyCreateInFlight = true;
    apiKeyCreateError = null;
    try {
      const result = await createApiKey(name);
      newKeyToken = result.token;
      await refreshApiKeys();
    } catch (err) {
      apiKeyCreateError = describe(err);
    } finally {
      apiKeyCreateInFlight = false;
    }
  }

  async function copyApiKeyToken() {
    if (!newKeyToken) return;
    try {
      await navigator.clipboard.writeText(newKeyToken);
      newKeyTokenCopied = true;
    } catch {
      // clipboard unavailable
    }
  }

  async function revokeKey(id: string) {
    if (!window.confirm('Revoke this API key? Existing scripts using it will stop working.'))
      return;
    revokingKeyId = id;
    apiKeyRevokeError = null;
    try {
      await revokeApiKey(id);
      await refreshApiKeys();
    } catch (err) {
      apiKeyRevokeError = describe(err);
    } finally {
      revokingKeyId = null;
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

  onMount(() => {
    refreshPasskeys();
    refreshApiKeys();
  });
</script>

<svelte:head>
  <title>Security — Kubinate</title>
</svelte:head>

<h1 class="text-2xl font-semibold mb-6">Security</h1>

<!-- MFA assertion prompt (only when ?mfa=required) -->
{#if mfaRequired}
  <div
    class="mb-6 rounded-lg border border-amber-300 bg-amber-50 dark:border-amber-700 dark:bg-amber-950/40 p-5"
    aria-live="polite"
  >
    <div class="flex items-start gap-3">
      <ShieldCheck class="mt-0.5 h-5 w-5 shrink-0 text-amber-600 dark:text-amber-400" />
      <div class="flex-1">
        <h2 class="text-sm font-semibold text-amber-900 dark:text-amber-200 mb-1">
          Verify your identity
        </h2>
        <p class="text-sm text-amber-800 dark:text-amber-300 mb-4">
          Use your passkey to complete sign-in.
        </p>
        <div class="flex flex-wrap items-center gap-3">
          <Button onclick={assert} disabled={assertInFlight} data-testid="assert-button">
            {assertInFlight ? 'Verifying…' : 'Authenticate with passkey'}
          </Button>
          <button
            type="button"
            onclick={openRedeemModal}
            class="text-sm text-amber-700 dark:text-amber-400 underline underline-offset-2 hover:text-amber-900 dark:hover:text-amber-200"
            data-testid="recovery-redeem-link"
          >
            Use a recovery code
          </button>
        </div>
        {#if assertError}
          <div
            class="mt-3 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
            role="alert"
          >
            {assertError}
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- Passkeys section -->
<Card class="mb-6">
  <CardHeader>
    <CardTitle>Passkeys</CardTitle>
    <CardDescription>
      Use a passkey to verify your identity when signing in as Owner or Admin.
    </CardDescription>
  </CardHeader>
  <CardContent class="space-y-4">
    {#if loadError}
      <div
        class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
        role="alert"
      >
        {loadError}
      </div>
    {:else if passkeys === null}
      <div class="space-y-2">
        {#each [1, 2] as _ (_)}
          <div class="h-10 rounded-md bg-muted animate-pulse"></div>
        {/each}
      </div>
    {:else if passkeys.length === 0}
      <div class="flex flex-col items-center justify-center py-8 text-center">
        <KeyRound class="h-8 w-8 text-muted-foreground mb-3" />
        <p class="text-sm text-muted-foreground">No passkeys registered yet.</p>
      </div>
    {:else}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Nickname</TableHead>
            <TableHead>Registered</TableHead>
            <TableHead>Last used</TableHead>
            <TableHead class="w-[100px]">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {#each passkeys as p (p.id)}
            <TableRow data-testid={`passkey-row-${p.id}`}>
              <TableCell class="font-medium text-sm">{p.nickname}</TableCell>
              <TableCell class="text-sm text-muted-foreground">{fmtDate(p.registered_at)}</TableCell
              >
              <TableCell class="text-sm text-muted-foreground"
                >{fmtLastUsed(p.last_used_at)}</TableCell
              >
              <TableCell>
                <Button
                  variant="destructive"
                  size="sm"
                  onclick={() => revoke(p.id, p.nickname)}
                  disabled={revokingId === p.id}
                  data-testid={`revoke-${p.id}`}
                >
                  {revokingId === p.id ? 'Revoking…' : 'Revoke'}
                </Button>
              </TableCell>
            </TableRow>
          {/each}
        </TableBody>
      </Table>
    {/if}

    {#if revokeError}
      <div
        class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
        role="alert"
      >
        {revokeError}
      </div>
    {/if}

    <div class="pt-2">
      <Button variant="outline" onclick={openRegisterModal} data-testid="register-button">
        <Plus class="mr-2 h-4 w-4" />
        Register a passkey
      </Button>
    </div>
  </CardContent>
</Card>

<!-- Recovery codes section -->
<Card>
  <CardHeader>
    <CardTitle>Recovery codes</CardTitle>
    <CardDescription>
      Recovery codes let you regain access if you lose your passkey.
    </CardDescription>
  </CardHeader>
  <CardContent class="space-y-4">
    <Button
      variant="outline"
      onclick={regenerate}
      disabled={recoveryInFlight}
      data-testid="regenerate-button"
    >
      <RefreshCw class="mr-2 h-4 w-4 {recoveryInFlight ? 'animate-spin' : ''}" />
      {recoveryInFlight ? 'Generating…' : 'Regenerate recovery codes'}
    </Button>

    {#if recoveryError && !recoveryModalOpen}
      <div
        class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
        role="alert"
      >
        {recoveryError}
      </div>
    {/if}
  </CardContent>
</Card>

<!-- API keys section -->
<Card class="mt-6">
  <CardHeader class="flex flex-row items-center justify-between">
    <div>
      <CardTitle>API keys</CardTitle>
      <CardDescription>
        Personal access tokens for programmatic access. Each token is shown once.
      </CardDescription>
    </div>
    <Button variant="outline" onclick={openApiKeyCreateModal} data-testid="create-api-key-button">
      <Plus class="mr-2 h-4 w-4" /> Create key
    </Button>
  </CardHeader>
  <CardContent class="space-y-4">
    {#if apiKeyLoadError}
      <div
        class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
        role="alert"
      >
        {apiKeyLoadError}
      </div>
    {:else if apiKeys === null}
      <div class="space-y-2">
        {#each [1, 2] as _ (_)}
          <div class="h-10 rounded-md bg-muted animate-pulse"></div>
        {/each}
      </div>
    {:else if apiKeys.length === 0}
      <div class="flex flex-col items-center justify-center py-8 text-center">
        <KeyRound class="h-8 w-8 text-muted-foreground mb-3" />
        <p class="text-sm text-muted-foreground">No API keys yet.</p>
      </div>
    {:else}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Prefix</TableHead>
            <TableHead>Created</TableHead>
            <TableHead>Last used</TableHead>
            <TableHead class="w-[100px]">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {#each apiKeys as k (k.id)}
            <TableRow data-testid={`api-key-row-${k.id}`}>
              <TableCell class="font-medium text-sm">{k.name}</TableCell>
              <TableCell class="font-mono text-sm text-muted-foreground">{k.token_prefix}</TableCell
              >
              <TableCell class="text-sm text-muted-foreground"
                >{new Date(k.created_at).toLocaleDateString()}</TableCell
              >
              <TableCell class="text-sm text-muted-foreground">
                {k.last_used_at ? new Date(k.last_used_at).toLocaleString() : 'never'}
              </TableCell>
              <TableCell>
                <Button
                  variant="destructive"
                  size="sm"
                  onclick={() => revokeKey(k.id)}
                  disabled={revokingKeyId === k.id}
                  data-testid={`revoke-key-${k.id}`}
                >
                  {revokingKeyId === k.id ? 'Revoking…' : 'Revoke'}
                </Button>
              </TableCell>
            </TableRow>
          {/each}
        </TableBody>
      </Table>
    {/if}

    {#if apiKeyRevokeError}
      <div
        class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
        role="alert"
      >
        {apiKeyRevokeError}
      </div>
    {/if}
  </CardContent>
</Card>

<!-- Register passkey modal -->
<Dialog bind:open={registerModalOpen}>
  <DialogContent>
    <DialogHeader>
      <DialogTitle>Register a passkey</DialogTitle>
      <DialogDescription>
        Pick a name that helps you recognise this device later (e.g. "MacBook" or "YubiKey 5C").
      </DialogDescription>
    </DialogHeader>

    <div class="space-y-3 py-2">
      <div class="space-y-1.5">
        <Label for="register-nickname">Nickname</Label>
        <Input
          id="register-nickname"
          type="text"
          bind:value={registerNickname}
          maxlength={64}
          required
          data-testid="register-nickname"
        />
      </div>

      {#if registerError}
        <div
          class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          role="alert"
        >
          {registerError}
        </div>
      {/if}
    </div>

    <DialogFooter>
      <Button variant="outline" onclick={closeRegisterModal} disabled={registerInFlight}>
        Cancel
      </Button>
      <Button onclick={confirmRegister} disabled={registerInFlight} data-testid="register-confirm">
        {registerInFlight ? 'Registering…' : 'Register'}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

<!-- Recovery codes display modal -->
<Dialog
  open={recoveryModalOpen && recoveryCodes !== null}
  onOpenChange={(open) => {
    if (!open) tryCloseRecoveryModal();
  }}
>
  <DialogContent data-testid="recovery-modal">
    <DialogHeader>
      <DialogTitle>Save your recovery codes</DialogTitle>
      <DialogDescription>
        Save these codes somewhere safe. Each code can only be used once.
      </DialogDescription>
    </DialogHeader>

    <div class="space-y-3 py-2">
      {#if recoveryCodes && recoveryCodes.length > 0}
        <div
          class="rounded-md border border-amber-200 bg-amber-50 dark:border-amber-800 dark:bg-amber-950/40 p-3"
        >
          <p class="text-xs font-medium text-amber-800 dark:text-amber-300 mb-2">
            These codes will not be shown again. Save them in your password manager now.
          </p>
          <div class="grid grid-cols-2 gap-1.5" data-testid="recovery-codes-list">
            {#each recoveryCodes as code (code)}
              <code
                class="block rounded bg-white dark:bg-zinc-900 border border-amber-200 dark:border-amber-800 px-2 py-1 font-mono text-sm text-center"
              >
                {code}
              </code>
            {/each}
          </div>
        </div>
      {/if}

      {#if recoveryError}
        <div
          class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          role="alert"
        >
          {recoveryError}
        </div>
      {/if}
    </div>

    <DialogFooter>
      <Button variant="outline" onclick={tryCloseRecoveryModal}>Close</Button>
      <Button onclick={copyCodes} data-testid="copy-codes">
        {recoveryCopied ? 'Copied' : 'Copy all'}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>

<!-- Create API key modal -->
<Dialog bind:open={apiKeyCreateOpen}>
  <DialogContent>
    <DialogHeader>
      <DialogTitle>Create API key</DialogTitle>
      <DialogDescription>
        Give this key a name so you can identify it later. The token is shown only once.
      </DialogDescription>
    </DialogHeader>

    {#if newKeyToken === null}
      <div class="space-y-3 py-2">
        <div class="space-y-1.5">
          <Label for="api-key-name">Name</Label>
          <Input
            id="api-key-name"
            type="text"
            bind:value={apiKeyName}
            placeholder="e.g. CI pipeline"
            data-testid="api-key-name"
          />
        </div>

        {#if apiKeyCreateError}
          <div
            class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
            role="alert"
          >
            {apiKeyCreateError}
          </div>
        {/if}
      </div>

      <DialogFooter>
        <Button variant="outline" onclick={closeApiKeyCreateModal} disabled={apiKeyCreateInFlight}>
          Cancel
        </Button>
        <Button
          onclick={confirmCreateApiKey}
          disabled={apiKeyCreateInFlight}
          data-testid="api-key-create-confirm"
        >
          {apiKeyCreateInFlight ? 'Creating…' : 'Create'}
        </Button>
      </DialogFooter>
    {:else}
      <div class="space-y-3 py-2">
        <div
          class="rounded-md border border-amber-200 bg-amber-50 dark:border-amber-800 dark:bg-amber-950/40 p-3"
        >
          <p class="text-xs font-medium text-amber-800 dark:text-amber-300 mb-2">
            This token will not be shown again.
          </p>
          <code
            class="block rounded bg-white dark:bg-zinc-900 border border-amber-200 dark:border-amber-800 px-3 py-2 font-mono text-sm break-all"
            data-testid="new-api-key-token"
          >
            {newKeyToken}
          </code>
        </div>
      </div>

      <DialogFooter>
        <Button variant="outline" onclick={copyApiKeyToken} data-testid="copy-api-key-token">
          {newKeyTokenCopied ? 'Copied' : 'Copy'}
        </Button>
        <Button onclick={closeApiKeyCreateModal} data-testid="api-key-done">Done</Button>
      </DialogFooter>
    {/if}
  </DialogContent>
</Dialog>

<!-- Recovery code redeem modal -->
<Dialog bind:open={redeemModalOpen}>
  <DialogContent>
    <DialogHeader>
      <DialogTitle>Use a recovery code</DialogTitle>
      <DialogDescription>
        Each code works once. After redeeming, regenerate a fresh batch.
      </DialogDescription>
    </DialogHeader>

    <div class="space-y-3 py-2">
      <div class="space-y-1.5">
        <Label for="recovery-code-input">Recovery code</Label>
        <Input
          id="recovery-code-input"
          type="text"
          bind:value={redeemCode}
          placeholder="AB12C-D3E4F"
          autocomplete="one-time-code"
          spellcheck={false}
          data-testid="recovery-code-input"
        />
      </div>

      {#if redeemError}
        <div
          class="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive"
          role="alert"
        >
          {redeemError}
        </div>
      {/if}
    </div>

    <DialogFooter>
      <Button variant="outline" onclick={closeRedeemModal} disabled={redeemInFlight}>Cancel</Button>
      <Button
        onclick={confirmRedeem}
        disabled={redeemInFlight}
        data-testid="recovery-redeem-confirm"
      >
        {redeemInFlight ? 'Redeeming…' : 'Redeem'}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
