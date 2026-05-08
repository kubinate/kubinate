<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';
  import { createCluster, loadClusterCatalog, ApiError } from '$lib/api/clusters';
  import { listCredentials } from '$lib/api/credentials';
  import {
    createClusterSchema,
    FALLBACK_REGIONS,
    FALLBACK_SERVER_TYPES,
    type CredentialView
  } from '$lib/api/schemas';

  // Form state. We let the user edit freely and validate on submit —
  // inline validation can come later if users ask for it.
  let name = $state('');
  let region = $state<string>(FALLBACK_REGIONS[0]);
  let serverType = $state<string>(FALLBACK_SERVER_TYPES[0]);
  let workerCount = $state(3);
  let credentialId = $state('');

  let regions = $state<string[]>([...FALLBACK_REGIONS]);
  let serverTypes = $state<string[]>([...FALLBACK_SERVER_TYPES]);

  // `null` means "list not yet loaded"; an empty array means "loaded
  // but the org has no credentials". The two render very differently,
  // so we keep them distinguishable.
  let credentials = $state<CredentialView[] | null>(null);

  let fieldErrors = $state<Record<string, string>>({});
  let submitError = $state<string | null>(null);
  let submitting = $state(false);

  onMount(async () => {
    try {
      const catalog = await loadClusterCatalog();
      regions = catalog.regions;
      serverTypes = catalog.server_types;
      if (!regions.includes(region)) region = regions[0];
      if (!serverTypes.includes(serverType)) serverType = serverTypes[0];
    } catch {
      // Keep the fallback list; the Rust service will still validate.
    }
    try {
      const list = await listCredentials();
      credentials = list;
      // Default to the first credential so a one-credential org gets a
      // useful pre-filled value without an extra click.
      if (list.length > 0 && !credentialId) {
        credentialId = list[0].id;
      }
    } catch {
      // On fetch failure leave `credentials` as `null` and let the user
      // see the loading state until they refresh.
    }
  });

  async function onSubmit(event: SubmitEvent) {
    event.preventDefault();
    fieldErrors = {};
    submitError = null;

    const parsed = createClusterSchema.safeParse({
      name,
      region,
      server_type: serverType,
      control_plane_count: 1,
      worker_count: workerCount,
      credential_id: credentialId
    });
    if (!parsed.success) {
      const next: Record<string, string> = {};
      for (const issue of parsed.error.issues) {
        next[issue.path.join('.')] = issue.message;
      }
      fieldErrors = next;
      return;
    }

    submitting = true;
    try {
      const cluster = await createCluster(parsed.data);
      await goto(`/app/clusters/${cluster.id}`);
    } catch (err) {
      submitError =
        err instanceof ApiError
          ? `${err.title}: ${err.detail}`
          : err instanceof Error
            ? err.message
            : 'Unknown error';
    } finally {
      submitting = false;
    }
  }
</script>

<svelte:head>
  <title>New cluster — Kubinate</title>
</svelte:head>

<h1>Create a cluster</h1>
<p class="hint">
  Sprint 1 ships single-node control planes and 1–10 workers. High-availability control planes
  arrive in Phase 2.
</p>

<form onsubmit={onSubmit} novalidate>
  <label>
    Name
    <input type="text" bind:value={name} autocomplete="off" required />
    {#if fieldErrors.name}<small class="error">{fieldErrors.name}</small>{/if}
  </label>

  <label>
    Region
    <select bind:value={region}>
      {#each regions as r (r)}
        <option value={r}>{r}</option>
      {/each}
    </select>
    {#if fieldErrors.region}<small class="error">{fieldErrors.region}</small>{/if}
  </label>

  <label>
    Server type
    <select bind:value={serverType}>
      {#each serverTypes as t (t)}
        <option value={t}>{t}</option>
      {/each}
    </select>
    {#if fieldErrors.server_type}
      <small class="error">{fieldErrors.server_type}</small>
    {/if}
  </label>

  <label>
    Worker count
    <input type="number" min="1" max="10" bind:value={workerCount} required />
    {#if fieldErrors.worker_count}
      <small class="error">{fieldErrors.worker_count}</small>
    {/if}
  </label>

  <label>
    Hetzner credential
    {#if credentials === null}
      <p class="muted">Loading credentials…</p>
    {:else if credentials.length === 0}
      <p class="muted" data-testid="no-credentials">
        You don't have any Hetzner tokens yet —
        <a href="/app/settings/integrations">add one</a> to provision a cluster.
      </p>
    {:else}
      <select bind:value={credentialId} data-testid="credential-picker" required>
        {#each credentials as c (c.id)}
          <option value={c.id}>{c.alias}</option>
        {/each}
      </select>
    {/if}
    {#if fieldErrors.credential_id}
      <small class="error">{fieldErrors.credential_id}</small>
    {/if}
  </label>

  {#if submitError}
    <p class="error" role="alert">{submitError}</p>
  {/if}

  <button type="submit" disabled={submitting || credentials === null || credentials.length === 0}>
    {submitting ? 'Creating…' : 'Create cluster'}
  </button>
</form>

<style>
  h1 {
    font-size: 1.8rem;
    margin-bottom: 0.25rem;
  }
  .hint {
    color: #555;
    margin-bottom: 1.5rem;
  }
  form {
    display: grid;
    gap: 1rem;
    max-width: 440px;
  }
  label {
    display: grid;
    gap: 0.25rem;
    font-weight: 600;
  }
  input,
  select {
    padding: 0.5rem;
    font: inherit;
    border: 1px solid #bbb;
    border-radius: 4px;
    font-weight: 400;
  }
  .error {
    color: #b00020;
    font-weight: 400;
  }
  .muted {
    color: #555;
    font-weight: 400;
    margin: 0;
  }
  .muted a {
    color: #1a7f37;
  }
  button {
    padding: 0.6rem 1.2rem;
    background: #222;
    color: #fff;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  button[disabled] {
    opacity: 0.6;
    cursor: not-allowed;
  }
</style>
