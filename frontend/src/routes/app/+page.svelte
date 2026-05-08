<script lang="ts">
  import type { PageData } from './$types';
  import type { ClusterView } from '$lib/api/schemas';
  import { Card, CardHeader, CardTitle, CardContent } from '$lib/components/ui/card';
  import { Badge } from '$lib/components/ui/badge';
  import { Plus, Server } from 'lucide-svelte';

  let { data }: { data: PageData } = $props();

  const clusters: ClusterView[] = $derived(data.clusters);

  function badgeVariant(status: ClusterView['status']): 'default' | 'secondary' | undefined {
    if (status === 'ready') return 'default';
    if (status === 'pending' || status === 'provisioning' || status === 'destroying') return 'secondary';
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
    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
      {#each clusters as cluster (cluster.id)}
        <a href="/app/clusters/{cluster.id}" class="group block outline-none">
          <Card class="h-full transition-shadow group-hover:shadow-md group-focus-visible:ring-2 group-focus-visible:ring-ring">
            <CardHeader class="gap-2">
              <div class="flex items-start justify-between gap-2">
                <CardTitle class="font-semibold leading-snug">{cluster.name}</CardTitle>
                {#if isDestructive(cluster.status)}
                  <Badge class="shrink-0 bg-destructive text-white">{cluster.status}</Badge>
                {:else}
                  <Badge variant={badgeVariant(cluster.status)} class="shrink-0 capitalize">
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
