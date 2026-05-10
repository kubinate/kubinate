<script lang="ts">
  import { onMount } from 'svelte';
  import { getMe } from '$lib/api/me';
  import { listAuditLog, PAGE_SIZE, type AuditLogEntry } from '$lib/api/audit-log';
  import { Badge } from '$lib/components/ui/badge';
  import { Button } from '$lib/components/ui/button';
  import {
    Table,
    TableHeader,
    TableRow,
    TableHead,
    TableBody,
    TableCell
  } from '$lib/components/ui/table';
  import { Loader2 } from 'lucide-svelte';

  let entries = $state<AuditLogEntry[] | null>(null);
  let loadError = $state<string | null>(null);
  let orgId = $state('');
  let loadingMore = $state(false);
  let loadMoreError = $state<string | null>(null);
  let hasMore = $derived(
    entries !== null && entries.length > 0 && entries.length % PAGE_SIZE === 0
  );

  function prettyAction(action: string): string {
    return action.replace(/\./g, ' › ').replace(/_/g, ' ');
  }

  onMount(async () => {
    try {
      const me = await getMe();
      orgId = me.organization_id;
      entries = await listAuditLog(orgId);
    } catch (err) {
      loadError = err instanceof Error ? err.message : 'Failed to load';
      entries = [];
    }
  });

  async function loadMore() {
    if (!entries || entries.length === 0) return;
    const cursor = entries[entries.length - 1].id;
    loadingMore = true;
    loadMoreError = null;
    try {
      const next = await listAuditLog(orgId, { before: cursor });
      entries = [...entries, ...next];
    } catch (err) {
      loadMoreError = err instanceof Error ? err.message : 'Failed to load more';
    } finally {
      loadingMore = false;
    }
  }
</script>

<svelte:head>
  <title>Audit log — Kubinate</title>
</svelte:head>

<h1 class="text-2xl font-semibold mb-6">Audit log</h1>

{#if loadError}
  <div
    class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
    role="alert"
  >
    {loadError}
  </div>
{:else if entries === null}
  <div class="space-y-2">
    {#each [1, 2, 3] as _ (_)}
      <div class="h-10 rounded-md bg-muted animate-pulse"></div>
    {/each}
  </div>
{:else if entries.length === 0}
  <p class="text-sm text-muted-foreground">No audit events recorded yet.</p>
{:else}
  <Table>
    <TableHeader>
      <TableRow>
        <TableHead>Time</TableHead>
        <TableHead>Actor</TableHead>
        <TableHead>Action</TableHead>
        <TableHead>Resource</TableHead>
        <TableHead>Decision</TableHead>
      </TableRow>
    </TableHeader>
    <TableBody>
      {#each entries as entry (entry.id)}
        <TableRow>
          <TableCell class="text-xs text-muted-foreground whitespace-nowrap">
            {new Date(entry.created_at).toLocaleString()}
          </TableCell>
          <TableCell class="text-sm">
            {entry.actor_display_name ?? entry.actor_email ?? '(system)'}
          </TableCell>
          <TableCell class="text-sm font-medium">
            {prettyAction(entry.action)}
          </TableCell>
          <TableCell class="text-xs text-muted-foreground font-mono">
            {entry.resource_type}{entry.resource_id ? ' ' + entry.resource_id.slice(0, 8) : ''}
          </TableCell>
          <TableCell>
            <Badge
              class={entry.decision === 'allowed'
                ? 'bg-emerald-100 text-emerald-800 border-emerald-200 text-xs'
                : 'bg-red-100 text-red-800 border-red-200 text-xs'}
            >
              {entry.decision}
            </Badge>
          </TableCell>
        </TableRow>
      {/each}
    </TableBody>
  </Table>

  {#if loadMoreError}
    <div
      class="rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive mt-4"
      role="alert"
    >
      {loadMoreError}
    </div>
  {/if}

  {#if hasMore}
    <div class="mt-4 flex justify-center">
      <Button variant="outline" onclick={loadMore} disabled={loadingMore}>
        {#if loadingMore}
          <Loader2 class="mr-2 h-4 w-4 animate-spin" />
          Loading…
        {:else}
          Load more
        {/if}
      </Button>
    </div>
  {/if}
{/if}
