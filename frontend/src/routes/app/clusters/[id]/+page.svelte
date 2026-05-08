<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { onDestroy, onMount } from 'svelte';
  import { ApiError, getCluster, loadClusterCatalog } from '$lib/api/clusters';
  import { installAddon, listAddons } from '$lib/api/addons';
  import { errorCategoryMessage, type AddonView, type ClusterView } from '$lib/api/schemas';

  // Sprint 3 ticket 10 — SSE is the primary live-update mechanism;
  // 2s polling is only a fallback for environments where the
  // EventSource fails to stay connected (corp proxies, restrictive
  // ad blockers, the rare case where Cloudflare buffers despite
  // the keep-alive header).
  const POLL_INTERVAL_MS = 2000;
  // After this many SSE reconnect attempts we give up and fall back
  // to polling. Each attempt is followed by exponential backoff so
  // the wall-clock cost of "give up" is on the order of 30s.
  const SSE_FALLBACK_AFTER_RETRIES = 3;
  const SSE_BACKOFF_INITIAL_MS = 1_000;
  const SSE_BACKOFF_MAX_MS = 60_000;

  let cluster = $state<ClusterView | null>(null);
  let error = $state<string | null>(null);
  let nowTick = $state(Date.now());
  let pollHandle: ReturnType<typeof setInterval> | null = null;
  let tickHandle: ReturnType<typeof setInterval> | null = null;
  let eventSource: EventSource | null = null;
  let reconnectHandle: ReturnType<typeof setTimeout> | null = null;
  let sseRetries = 0;
  let sseAbandoned = false;

  // Sprint 3 ticket 03 — addon panel state.
  let addons = $state<AddonView[] | null>(null);
  let availableAddons = $state<string[]>([]);
  let installModalOpen = $state(false);
  let installInFlight = $state(false);
  let installError = $state<string | null>(null);
  let installAddonSlug = $state<string | null>(null);
  let installVersion = $state('');

  async function refresh() {
    try {
      cluster = await getCluster(page.params.id!);
      error = null;

      // Once the cluster is Ready the API accepts addon installs;
      // before that the addon list endpoint still works (returns
      // empty) but a parallel fetch keeps the panel responsive on
      // first reach-Ready.
      const list = await listAddons(page.params.id!);
      addons = list;

      if (cluster.terminal) {
        stopPolling();
        closeEventSource();
      }
    } catch (err) {
      error =
        err instanceof ApiError
          ? `${err.title}: ${err.detail}`
          : err instanceof Error
            ? err.message
            : 'Unknown error';
    }
  }

  async function loadAvailableAddons() {
    try {
      const catalog = await loadClusterCatalog();
      availableAddons = catalog.addons;
    } catch {
      // Catalog fetch is best-effort; the install modal falls back
      // to manual slug entry if it ever comes back empty.
    }
  }

  function openInstallModal(slug: string) {
    installAddonSlug = slug;
    installVersion = defaultVersionFor(slug);
    installError = null;
    installModalOpen = true;
  }

  function closeInstallModal() {
    installModalOpen = false;
    installAddonSlug = null;
  }

  function defaultVersionFor(slug: string): string {
    // Per Sprint 2 ticket 07's catalog, version is required on submit;
    // suggesting a known-good chart version mirrors the form pattern
    // from the cluster-create page.
    switch (slug) {
      case 'ingress-nginx':
        return '4.10.0';
      case 'cert-manager':
        return '1.16.0';
      default:
        return '';
    }
  }

  async function confirmInstall() {
    if (!installAddonSlug || !cluster) return;
    if (installVersion.trim().length === 0) {
      installError = 'Version is required';
      return;
    }
    installInFlight = true;
    installError = null;
    try {
      await installAddon(cluster.id, installAddonSlug, installVersion.trim());
      closeInstallModal();
      // Re-fetch immediately so the new row's `installing` state
      // shows up before the next 2s poll.
      await refresh();
    } catch (err) {
      installError =
        err instanceof ApiError
          ? `${err.title}: ${err.detail}`
          : err instanceof Error
            ? err.message
            : 'Unknown error';
    } finally {
      installInFlight = false;
    }
  }

  function describeAddonStatus(addon: AddonView): string {
    if (addon.status === 'failed' && addon.status_reason) {
      // status_reason has already been sanitised on the API side
      // (Sprint 1 ticket 06's error_category mapper). Render as-is —
      // raw Helm strings never reach the UI.
      return addon.status_reason;
    }
    return addon.status;
  }

  // Hide the "Install …" CTA for slugs that already have an active
  // (non-uninstalled) row.
  const installableSlugs = $derived.by<string[]>(() => {
    if (!availableAddons.length) return [];
    if (!addons) return availableAddons;
    const taken = new Set(addons.filter((a) => a.status !== 'uninstalled').map((a) => a.addon));
    return availableAddons.filter((s) => !taken.has(s));
  });

  function stopPolling() {
    if (pollHandle) {
      clearInterval(pollHandle);
      pollHandle = null;
    }
  }

  function startPolling() {
    if (pollHandle) return;
    pollHandle = setInterval(refresh, POLL_INTERVAL_MS);
  }

  function closeEventSource() {
    if (reconnectHandle) {
      clearTimeout(reconnectHandle);
      reconnectHandle = null;
    }
    if (eventSource) {
      eventSource.close();
      eventSource = null;
    }
  }

  function fallBackToPolling() {
    sseAbandoned = true;
    closeEventSource();
    startPolling();
  }

  function scheduleReconnect() {
    sseRetries += 1;
    if (sseRetries > SSE_FALLBACK_AFTER_RETRIES) {
      fallBackToPolling();
      return;
    }
    // Exponential backoff with a 60s ceiling. Reset on a successful
    // `connected` event.
    const delay = Math.min(SSE_BACKOFF_INITIAL_MS * 2 ** (sseRetries - 1), SSE_BACKOFF_MAX_MS);
    reconnectHandle = setTimeout(openEventSource, delay);
  }

  function openEventSource() {
    reconnectHandle = null;
    if (sseAbandoned) return;
    if (typeof EventSource === 'undefined') {
      // Older runtimes don't ship EventSource; just poll.
      fallBackToPolling();
      return;
    }
    if (eventSource) {
      eventSource.close();
    }

    const id = page.params.id!;
    const es = new EventSource(`/api/v1/clusters/${id}/events`);
    eventSource = es;

    es.addEventListener('connected', () => {
      sseRetries = 0;
      // Successful SSE connection means polling is redundant.
      stopPolling();
    });

    es.addEventListener('step', () => {
      // Server has already written the shadow-table row before
      // publishing the SSE event, so a refetch sees the new step.
      refresh();
    });

    es.addEventListener('terminal', () => {
      // Pull final state once, then close — the server will close
      // its end too, but doing it explicitly here avoids the brief
      // window where the browser would otherwise try to reconnect.
      refresh();
      closeEventSource();
    });

    es.onerror = () => {
      // EventSource fires `error` on both transient drops and on
      // server-initiated close. Close + schedule a reconnect; the
      // backoff handler decides whether to give up.
      if (eventSource === es) {
        es.close();
        eventSource = null;
        scheduleReconnect();
      }
    };
  }

  onMount(() => {
    refresh();
    loadAvailableAddons();
    openEventSource();
    // Separate ticker so the "elapsed" display updates every second
    // without triggering an extra fetch.
    tickHandle = setInterval(() => {
      nowTick = Date.now();
    }, 1000);
  });

  onDestroy(() => {
    stopPolling();
    closeEventSource();
    if (tickHandle) clearInterval(tickHandle);
  });

  // Sprint 1 thin slice: 5–15 minute provisioning per ADR-0003 §Context.
  // Use the midpoint as a coarse "estimated remaining" until the
  // workflow records per-step durations.
  const TYPICAL_PROVISION_MS = 10 * 60 * 1000;

  const elapsedSeconds = $derived.by(() => {
    if (!cluster) return 0;
    const started = new Date(cluster.started_at).getTime();
    return Math.max(0, Math.floor((nowTick - started) / 1000));
  });

  const estimatedRemainingSeconds = $derived.by(() => {
    if (!cluster) return 0;
    if (cluster.terminal) return 0;
    const started = new Date(cluster.started_at).getTime();
    const remaining = TYPICAL_PROVISION_MS - (nowTick - started);
    return Math.max(0, Math.floor(remaining / 1000));
  });

  function fmtMmSs(seconds: number): string {
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return `${m}m ${s.toString().padStart(2, '0')}s`;
  }

  function prettyStep(step: string | null | undefined): string | null {
    if (!step) return null;
    return step.replace(/_/g, ' ');
  }

  async function retry() {
    // Wires to ticket 05's destroy + re-trigger ticket 03's runner.
    // Until that runner is in place this just navigates back to the
    // create form so the user can submit again.
    await goto('/app/clusters/new');
  }
</script>

<svelte:head>
  <title>Cluster — Kubinate</title>
</svelte:head>

<h1>Cluster {cluster?.name ?? page.params.id}</h1>

{#if error}
  <p class="error" role="alert">{error}</p>
{:else if !cluster}
  <p>Loading…</p>
{:else}
  <dl>
    <dt>Status</dt>
    <dd class="status status-{cluster.status}">{cluster.status}</dd>

    {#if cluster.current_step}
      <dt>Step</dt>
      <dd>{prettyStep(cluster.current_step)}</dd>
    {/if}

    <dt>Region</dt>
    <dd>{cluster.region}</dd>
    <dt>Server type</dt>
    <dd>{cluster.server_type}</dd>
    <dt>Workers</dt>
    <dd>{cluster.worker_count}</dd>
    <dt>Created</dt>
    <dd>{new Date(cluster.created_at).toLocaleString()}</dd>

    {#if !cluster.terminal}
      <dt>Elapsed</dt>
      <dd>{fmtMmSs(elapsedSeconds)}</dd>
      <dt>Approx. remaining</dt>
      <dd>{fmtMmSs(estimatedRemainingSeconds)}</dd>
    {/if}
  </dl>

  {#if cluster.status === 'pending' || cluster.status === 'provisioning'}
    <p class="progress" aria-live="polite">
      Provisioning typically takes 5–15 minutes. This page refreshes every 2&nbsp;seconds.
    </p>
  {:else if cluster.status === 'ready' && cluster.kubeconfig_available}
    <a class="cta" href={`/api/v1/clusters/${cluster.id}/kubeconfig`} download>
      Download kubeconfig
    </a>
  {:else if cluster.status === 'failed' && cluster.error_category}
    {@const message = errorCategoryMessage(cluster.error_category)}
    <section class="failure">
      <h2>{message.title}</h2>
      <p>{message.detail}</p>
      {#if message.retryable}
        <button type="button" onclick={retry}>Retry</button>
      {/if}
    </section>
  {/if}

  <section class="addons" data-testid="addon-panel">
    <h2>Add-ons</h2>
    {#if addons === null}
      <p class="muted">Loading…</p>
    {:else if addons.length === 0}
      <p class="muted">No add-ons installed yet.</p>
    {:else}
      <table>
        <thead>
          <tr>
            <th>Add-on</th>
            <th>Version</th>
            <th>Status</th>
          </tr>
        </thead>
        <tbody>
          {#each addons as a (a.id)}
            <tr data-testid={`addon-row-${a.addon}`}>
              <td>{a.addon}</td>
              <td>{a.version}</td>
              <td class="status status-{a.status}">{describeAddonStatus(a)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    {#if cluster.status === 'ready' && installableSlugs.length > 0}
      <div class="addon-ctas" data-testid="addon-ctas">
        {#each installableSlugs as slug (slug)}
          <button
            type="button"
            onclick={() => openInstallModal(slug)}
            data-testid={`install-${slug}`}
          >
            Install {slug}
          </button>
        {/each}
      </div>
    {/if}
  </section>
{/if}

{#if installModalOpen && installAddonSlug}
  <div class="modal-backdrop" role="dialog" aria-modal="true" data-testid="install-modal">
    <div class="modal">
      <h3>Install {installAddonSlug}</h3>
      <p class="muted">
        Pinned chart version. Older + newer versions need a separate operator review before they hit
        the catalog.
      </p>
      <label>
        Version
        <input type="text" bind:value={installVersion} data-testid="install-version" required />
      </label>
      {#if installError}
        <p class="error" role="alert">{installError}</p>
      {/if}
      <div class="modal-actions">
        <button type="button" onclick={closeInstallModal} disabled={installInFlight}>
          Cancel
        </button>
        <button
          type="button"
          onclick={confirmInstall}
          disabled={installInFlight}
          data-testid="install-confirm"
        >
          {installInFlight ? 'Installing…' : 'Install'}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.5rem 1.5rem;
  }
  dt {
    font-weight: 600;
    color: #333;
  }
  .status {
    text-transform: capitalize;
    font-weight: 600;
  }
  .status-pending,
  .status-provisioning,
  .status-destroying {
    color: #b36b00;
  }
  .status-ready {
    color: #1a7f37;
  }
  .status-failed,
  .status-destroyed {
    color: #b00020;
  }
  .progress {
    color: #555;
    margin-top: 1rem;
  }
  .error {
    color: #b00020;
  }
  .cta {
    display: inline-block;
    margin-top: 1rem;
    padding: 0.6rem 1.2rem;
    background: #1a7f37;
    color: #fff;
    border-radius: 4px;
    text-decoration: none;
    font-weight: 600;
  }
  .failure {
    margin-top: 1.5rem;
    padding: 1rem;
    background: #fff5f5;
    border: 1px solid #f5b3b3;
    border-radius: 6px;
  }
  .failure h2 {
    margin: 0 0 0.5rem 0;
    font-size: 1.1rem;
    color: #b00020;
  }
  .failure button {
    padding: 0.5rem 1rem;
    background: #222;
    color: #fff;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  .addons {
    margin-top: 2rem;
  }
  .addons h2 {
    font-size: 1.1rem;
    margin: 0 0 0.5rem;
  }
  .addons table {
    width: 100%;
    border-collapse: collapse;
    margin-bottom: 0.75rem;
  }
  .addons th,
  .addons td {
    text-align: left;
    padding: 0.4rem 0.5rem;
    border-bottom: 1px solid #eee;
  }
  .addons th {
    font-size: 0.85rem;
    color: #555;
    font-weight: 600;
  }
  .addon-ctas {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .addon-ctas button {
    padding: 0.5rem 1rem;
    background: #1a7f37;
    color: #fff;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  .muted {
    color: #555;
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
    width: min(420px, 90vw);
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
