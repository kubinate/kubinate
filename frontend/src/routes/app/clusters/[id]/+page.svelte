<script lang="ts">
  import { page } from '$app/state';
  import { goto } from '$app/navigation';
  import { onDestroy, onMount } from 'svelte';
  import { ApiError, getCluster, loadClusterCatalog } from '$lib/api/clusters';
  import { installAddon, listAddons } from '$lib/api/addons';
  import { errorCategoryMessage, type AddonView, type ClusterView } from '$lib/api/schemas';
  import { Download, Loader2 } from 'lucide-svelte';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import { Card, CardHeader, CardTitle, CardContent } from '$lib/components/ui/card';
  import {
    Table,
    TableHeader,
    TableRow,
    TableHead,
    TableBody,
    TableCell
  } from '$lib/components/ui/table';
  import {
    Dialog,
    DialogContent,
    DialogHeader,
    DialogTitle,
    DialogDescription,
    DialogFooter
  } from '$lib/components/ui/dialog';
  import { Input } from '$lib/components/ui/input';
  import { Skeleton } from '$lib/components/ui/skeleton';

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

  function statusBadgeClass(status: string): string {
    if (status === 'ready') {
      return 'bg-emerald-100 text-emerald-800 border-emerald-200';
    }
    if (status === 'failed' || status === 'destroyed') {
      return 'bg-red-100 text-red-800 border-red-200';
    }
    return 'bg-amber-100 text-amber-800 border-amber-200';
  }

  function addonStatusBadgeClass(status: string): string {
    if (status === 'ready') {
      return 'bg-emerald-100 text-emerald-800 border-emerald-200';
    }
    if (status === 'failed') {
      return 'bg-red-100 text-red-800 border-red-200';
    }
    return 'bg-amber-100 text-amber-800 border-amber-200';
  }
</script>

<svelte:head>
  <title>Cluster — Kubinate</title>
</svelte:head>

{#if error}
  <div
    role="alert"
    class="rounded-md border border-destructive/20 bg-destructive/10 p-4 text-sm text-destructive"
  >
    {error}
  </div>
{:else if !cluster}
  <div class="flex items-center justify-center py-24">
    <div class="h-8 w-8 animate-spin rounded-full border-4 border-muted border-t-foreground"></div>
  </div>
{:else}
  <div class="flex items-center gap-3 mb-6">
    <h1 class="text-2xl font-semibold">{cluster?.name ?? 'Cluster'}</h1>
    <Badge class={statusBadgeClass(cluster.status)}>{cluster.status}</Badge>
  </div>

  <div class="lg:grid lg:grid-cols-3 lg:gap-6">
    <!-- Left column -->
    <div class="lg:col-span-2 space-y-4">
      <!-- Status card -->
      <Card>
        <CardHeader>
          <CardTitle>Cluster details</CardTitle>
        </CardHeader>
        <CardContent>
          <dl class="grid grid-cols-2 gap-x-4 gap-y-3 text-sm">
            <dt class="font-medium text-muted-foreground">Region</dt>
            <dd>{cluster.region}</dd>

            <dt class="font-medium text-muted-foreground">Server type</dt>
            <dd>{cluster.server_type}</dd>

            <dt class="font-medium text-muted-foreground">Workers</dt>
            <dd>{cluster.worker_count}</dd>

            <dt class="font-medium text-muted-foreground">Created</dt>
            <dd>{new Date(cluster.created_at).toLocaleString()}</dd>

            <dt class="font-medium text-muted-foreground">Status</dt>
            <dd class="capitalize">{cluster.status}</dd>

            {#if cluster.current_step}
              <dt class="font-medium text-muted-foreground">Step</dt>
              <dd class="capitalize">{prettyStep(cluster.current_step)}</dd>
            {/if}

            {#if !cluster.terminal}
              <dt class="font-medium text-muted-foreground">Elapsed</dt>
              <dd>{fmtMmSs(elapsedSeconds)}</dd>

              <dt class="font-medium text-muted-foreground">Approx. remaining</dt>
              <dd>{fmtMmSs(estimatedRemainingSeconds)}</dd>
            {/if}
          </dl>
        </CardContent>
      </Card>

      <!-- Provisioning progress -->
      {#if cluster.status === 'pending' || cluster.status === 'provisioning'}
        <div class="rounded-lg border border-amber-200 bg-amber-50 p-4" aria-live="polite">
          <div class="flex items-start gap-3">
            <Loader2 class="h-4 w-4 animate-spin text-amber-600 mt-0.5 shrink-0" />
            <div>
              {#if cluster.current_step}
                <p class="text-sm font-medium text-amber-800 capitalize">
                  {prettyStep(cluster.current_step)}
                </p>
              {/if}
              <p class="text-sm text-amber-700 mt-1">Provisioning typically takes 5–15 minutes.</p>
            </div>
          </div>
        </div>
      {/if}

      <!-- Failure card -->
      {#if cluster.status === 'failed' && cluster.error_category}
        {@const message = errorCategoryMessage(cluster.error_category)}
        <div class="rounded-lg border border-red-200 bg-red-50 p-4">
          <h2 class="text-sm font-semibold text-red-800">{message.title}</h2>
          <p class="text-sm text-red-700 mt-1">{message.detail}</p>
          {#if message.retryable}
            <Button variant="outline" size="sm" onclick={retry} class="mt-3">Retry</Button>
          {/if}
        </div>
      {/if}

      <!-- Ready actions -->
      {#if cluster.status === 'ready' && cluster.kubeconfig_available}
        <div>
          <Button href={`/api/v1/clusters/${cluster.id}/kubeconfig`} download>
            <Download class="h-4 w-4" />
            Download kubeconfig
          </Button>
        </div>
      {/if}
    </div>

    <!-- Right column -->
    <div class="lg:col-span-1 mt-6 lg:mt-0">
      <Card data-testid="addon-panel">
        <CardHeader>
          <CardTitle>Add-ons</CardTitle>
        </CardHeader>
        <CardContent class="space-y-4">
          {#if addons === null}
            <div class="space-y-2">
              <Skeleton class="h-4 w-full" />
              <Skeleton class="h-4 w-3/4" />
              <Skeleton class="h-4 w-1/2" />
            </div>
          {:else if addons.length === 0}
            <p class="text-sm text-muted-foreground">No add-ons installed.</p>
          {:else}
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Add-on</TableHead>
                  <TableHead>Version</TableHead>
                  <TableHead>Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {#each addons as a (a.id)}
                  <TableRow data-testid={`addon-row-${a.addon}`}>
                    <TableCell class="font-medium">{a.addon}</TableCell>
                    <TableCell>{a.version}</TableCell>
                    <TableCell>
                      <Badge class={addonStatusBadgeClass(a.status)}>
                        {describeAddonStatus(a)}
                      </Badge>
                    </TableCell>
                  </TableRow>
                {/each}
              </TableBody>
            </Table>
          {/if}

          {#if cluster.status === 'ready' && installableSlugs.length > 0}
            <div class="flex flex-wrap gap-2" data-testid="addon-ctas">
              {#each installableSlugs as slug (slug)}
                <Button
                  variant="outline"
                  size="sm"
                  onclick={() => openInstallModal(slug)}
                  data-testid={`install-${slug}`}
                >
                  Install {slug}
                </Button>
              {/each}
            </div>
          {/if}
        </CardContent>
      </Card>
    </div>
  </div>
{/if}

<!-- Install modal -->
<Dialog bind:open={installModalOpen}>
  <DialogContent data-testid="install-modal">
    <DialogHeader>
      <DialogTitle>Install {installAddonSlug}</DialogTitle>
      <DialogDescription>
        Pinned chart version. Older + newer versions need a separate operator review before they hit
        the catalog.
      </DialogDescription>
    </DialogHeader>

    <div class="space-y-3 py-2">
      <div class="space-y-1.5">
        <label for="install-version" class="text-sm font-medium">Version</label>
        <Input
          id="install-version"
          type="text"
          bind:value={installVersion}
          data-testid="install-version"
          required
        />
      </div>

      {#if installError}
        <div
          role="alert"
          class="rounded-md border border-destructive/20 bg-destructive/10 p-3 text-sm text-destructive"
        >
          {installError}
        </div>
      {/if}
    </div>

    <DialogFooter>
      <Button variant="outline" onclick={closeInstallModal} disabled={installInFlight}>
        Cancel
      </Button>
      <Button onclick={confirmInstall} disabled={installInFlight} data-testid="install-confirm">
        {#if installInFlight}
          <Loader2 class="mr-2 h-4 w-4 animate-spin" />
          Installing…
        {:else}
          Install
        {/if}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
