<script lang="ts">
  import { onMount, onDestroy, untrack } from 'svelte';
  import type { PageData } from './$types';
  import type { ClusterView } from '$lib/api/schemas';
  import { listClusters } from '$lib/api/clusters';
  import { Card, CardHeader, CardTitle, CardContent } from '$lib/components/ui/card';
  import { Badge } from '$lib/components/ui/badge';
  import { Input } from '$lib/components/ui/input';
  import { Plus, Server } from 'lucide-svelte';
  import { SvelteMap, SvelteSet } from 'svelte/reactivity';

  let { data }: { data: PageData } = $props();

  const TRANSIENT = new Set(['pending', 'provisioning', 'scaling', 'destroying']);
  const POLL_MS = 3000;

  let clusters = $state<ClusterView[]>(untrack(() => data.clusters));
  let search = $state('');
  let activeStatuses = new SvelteSet<string>();
  let pollHandle: ReturnType<typeof setInterval> | null = null;

  function badgeVariant(status: ClusterView['status']): 'default' | 'secondary' | undefined {
    if (status === 'ready') return 'default';
    if (
      status === 'pending' ||
      status === 'provisioning' ||
      status === 'destroying' ||
      status === 'scaling'
    )
      return 'secondary';
    return undefined;
  }

  function isDestructive(status: ClusterView['status']): boolean {
    return status === 'failed' || status === 'destroyed';
  }

  function formatDate(iso: string): string {
    return new Date(iso).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric'
    });
  }

  function isAgentConnected(cluster: ClusterView): boolean {
    if (!cluster.agent_last_seen_at) return false;
    return Date.now() - new Date(cluster.agent_last_seen_at).getTime() < 90_000;
  }

  const activeClusters = $derived(clusters.filter((c) => c.status !== 'destroyed'));

  const totalWorkers = $derived.by(() =>
    activeClusters.reduce((sum, c) => sum + (c.worker_count ?? 0), 0)
  );

  const agentsOnline = $derived.by(
    () =>
      activeClusters.filter(
        (c) => (c.status === 'ready' || c.status === 'scaling') && isAgentConnected(c)
      ).length
  );

  const statusBreakdown = $derived.by(() => {
    const counts = new SvelteMap<string, number>();
    for (const c of activeClusters) {
      counts.set(c.status, (counts.get(c.status) ?? 0) + 1);
    }
    return [...counts.entries()]
      .filter(([, n]) => n > 0)
      .map(([s, n]) => `${n} ${s}`)
      .join(' · ');
  });

  const hasTransient = $derived(clusters.some((c) => TRANSIENT.has(c.status)));

  const availableStatuses = $derived.by(() => {
    const order: ClusterView['status'][] = [
      'provisioning',
      'scaling',
      'ready',
      'failed',
      'destroyed'
    ];
    const present = new Set(clusters.map((c) => c.status));
    return order.filter((s) => present.has(s));
  });

  const filteredClusters = $derived.by(() => {
    let list = clusters;
    if (search.trim()) {
      const q = search.trim().toLowerCase();
      list = list.filter((c) => c.name.toLowerCase().includes(q));
    }
    if (activeStatuses.size > 0) {
      list = list.filter((c) => activeStatuses.has(c.status));
    }
    return list;
  });

  function startPoll() {
    if (pollHandle) return;
    pollHandle = setInterval(async () => {
      clusters = await listClusters().catch(() => clusters);
    }, POLL_MS);
  }

  function stopPoll() {
    if (pollHandle) {
      clearInterval(pollHandle);
      pollHandle = null;
    }
  }

  $effect(() => {
    if (hasTransient) {
      startPoll();
    } else {
      stopPoll();
    }
  });

  function toggleStatus(s: string) {
    if (activeStatuses.has(s)) activeStatuses.delete(s);
    else activeStatuses.add(s);
  }

  onMount(async () => {
    clusters = await listClusters().catch(() => clusters);
  });

  onDestroy(() => {
    stopPoll();
  });
</script>

<div class="flex flex-col gap-6 p-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-semibold">Clusters</h1>
    <a
      href="/app/clusters/new"
      class="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
    >
      <Plus class="size-4" />
      New cluster
    </a>
  </div>

  {#if clusters.length === 0}
    <div class="flex flex-1 items-center justify-center py-24">
      <div class="flex flex-col items-center gap-4 text-center">
        <div class="flex size-16 items-center justify-center rounded-full bg-muted">
          <Server class="size-8 text-muted-foreground" />
        </div>
        <div class="flex flex-col gap-1">
          <h2 class="text-lg font-semibold">No clusters yet</h2>
          <p class="text-sm text-muted-foreground">
            Create your first k3s cluster on Hetzner infrastructure.
          </p>
        </div>
        <a
          href="/app/clusters/new"
          class="inline-flex items-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground transition-colors hover:bg-primary/90"
        >
          <Plus class="size-4" />
          Create cluster
        </a>
      </div>
    </div>
  {:else}
    <div class="grid grid-cols-2 gap-3 sm:grid-cols-4 mb-6">
      <div class="rounded-lg border border-border bg-card px-4 py-3">
        <p class="text-xs text-muted-foreground">Total clusters</p>
        <p class="text-xl font-semibold">{activeClusters.length}</p>
      </div>
      <div class="rounded-lg border border-border bg-card px-4 py-3">
        <p class="text-xs text-muted-foreground">Total workers</p>
        <p class="text-xl font-semibold">{totalWorkers}</p>
      </div>
      <div class="rounded-lg border border-border bg-card px-4 py-3">
        <p class="text-xs text-muted-foreground">Agents online</p>
        <p class="text-xl font-semibold">{agentsOnline}</p>
      </div>
      <div class="rounded-lg border border-border bg-card px-4 py-3">
        <p class="text-xs text-muted-foreground">Status</p>
        <p class="text-xl font-semibold truncate" title={statusBreakdown}>{statusBreakdown}</p>
      </div>
    </div>

    <div class="flex flex-col gap-3 mb-4">
      <Input bind:value={search} placeholder="Search clusters…" class="max-w-xs" />
      {#if availableStatuses.length > 1}
        <div class="flex flex-wrap gap-2">
          {#each availableStatuses as s (s)}
            <button
              onclick={() => toggleStatus(s)}
              class="rounded-full px-3 py-1 text-xs font-medium border transition-colors capitalize
                {activeStatuses.has(s)
                ? 'bg-primary text-primary-foreground border-primary'
                : 'bg-background text-muted-foreground border-border hover:border-foreground'}"
            >
              {s}
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#if filteredClusters.length === 0}
        <p class="text-sm text-muted-foreground py-8 text-center col-span-full">
          No clusters match your filter.
        </p>
      {/if}
      {#each filteredClusters as cluster (cluster.id)}
        <a href="/app/clusters/{cluster.id}" class="group block outline-none">
          <Card
            class="h-full transition-shadow group-hover:shadow-md group-focus-visible:ring-2 group-focus-visible:ring-ring"
          >
            <CardHeader class="gap-2">
              <div class="flex items-start justify-between gap-2">
                <CardTitle class="font-semibold leading-snug">{cluster.name}</CardTitle>
                {#if isDestructive(cluster.status)}
                  <Badge class="shrink-0 bg-destructive text-white">{cluster.status}</Badge>
                {:else}
                  <Badge
                    variant={badgeVariant(cluster.status)}
                    class="shrink-0 capitalize {TRANSIENT.has(cluster.status)
                      ? 'animate-pulse'
                      : ''}"
                  >
                    {cluster.status}
                  </Badge>
                {/if}
              </div>
            </CardHeader>
            <CardContent class="flex flex-col gap-3">
              <dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm text-muted-foreground">
                <dt class="font-medium text-foreground">Region</dt>
                <dd>{cluster.region}</dd>
                <dt class="font-medium text-foreground">Server type</dt>
                <dd>{cluster.server_type}</dd>
                <dt class="font-medium text-foreground">Workers</dt>
                <dd>{cluster.worker_count}</dd>
              </dl>
              {#if cluster.status === 'ready' || cluster.status === 'scaling'}
                {#if isAgentConnected(cluster)}
                  <span class="flex items-center gap-1.5 text-xs font-medium">
                    <span class="size-1.5 rounded-full bg-green-500"></span>
                    <span class="text-green-700 dark:text-green-400">Agent connected</span>
                  </span>
                {:else}
                  <span class="flex items-center gap-1.5 text-xs font-medium">
                    <span class="size-1.5 rounded-full bg-red-500"></span>
                    <span class="text-red-600 dark:text-red-400">Agent offline</span>
                  </span>
                {/if}
              {/if}
              <p class="text-xs text-muted-foreground">
                Created {formatDate(cluster.created_at)}
              </p>
            </CardContent>
          </Card>
        </a>
      {/each}
    </div>
  {/if}
</div>
