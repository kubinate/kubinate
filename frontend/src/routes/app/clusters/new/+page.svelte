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
  import { Card, CardContent } from '$lib/components/ui/card';
  import { Label } from '$lib/components/ui/label';
  import { Input } from '$lib/components/ui/input';
  import { Button } from '$lib/components/ui/button';
  import { Server, Package, Key } from 'lucide-svelte';

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

<h1 class="text-2xl font-semibold mb-1">New cluster</h1>
<p class="text-muted-foreground text-sm mb-8">Provision a k3s cluster on your Hetzner account.</p>

<div class="lg:grid lg:grid-cols-5 lg:gap-12">
  <!-- Form column -->
  <div class="lg:col-span-3">
    <Card>
      <CardContent class="pt-6">
        <form onsubmit={onSubmit} novalidate>
          <div class="space-y-5">
            <!-- Name -->
            <div class="space-y-1.5">
              <Label for="cluster-name">Name</Label>
              <Input
                id="cluster-name"
                type="text"
                bind:value={name}
                autocomplete="off"
                required
                placeholder="my-cluster"
              />
              {#if fieldErrors.name}
                <p class="text-sm text-destructive">{fieldErrors.name}</p>
              {/if}
            </div>

            <!-- Region -->
            <div class="space-y-1.5">
              <Label for="cluster-region">Region</Label>
              <select
                id="cluster-region"
                bind:value={region}
                class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              >
                {#each regions as r (r)}
                  <option value={r}>{r}</option>
                {/each}
              </select>
              {#if fieldErrors.region}
                <p class="text-sm text-destructive">{fieldErrors.region}</p>
              {/if}
            </div>

            <!-- Server type -->
            <div class="space-y-1.5">
              <Label for="cluster-server-type">Server type</Label>
              <select
                id="cluster-server-type"
                bind:value={serverType}
                class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              >
                {#each serverTypes as t (t)}
                  <option value={t}>{t}</option>
                {/each}
              </select>
              {#if fieldErrors.server_type}
                <p class="text-sm text-destructive">{fieldErrors.server_type}</p>
              {/if}
            </div>

            <!-- Worker count -->
            <div class="space-y-1.5">
              <Label for="cluster-worker-count">Worker count</Label>
              <select
                id="cluster-worker-count"
                bind:value={workerCount}
                class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
              >
                {#each [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as n (n)}
                  <option value={n}>{n}</option>
                {/each}
              </select>
              {#if fieldErrors.worker_count}
                <p class="text-sm text-destructive">{fieldErrors.worker_count}</p>
              {/if}
            </div>

            <!-- Hetzner credential -->
            <div class="space-y-1.5">
              <Label for="cluster-credential">Hetzner credential</Label>
              {#if credentials === null}
                <p class="text-sm text-muted-foreground">Loading credentials…</p>
              {:else if credentials.length === 0}
                <div class="rounded-md border border-amber-200 bg-amber-50 p-3 text-sm text-amber-800">
                  No Hetzner tokens yet — <a
                    href="/app/settings/integrations"
                    class="underline underline-offset-2 font-medium hover:text-amber-900"
                    >add one</a
                  > to provision a cluster.
                </div>
              {:else}
                <select
                  id="cluster-credential"
                  bind:value={credentialId}
                  data-testid="credential-picker"
                  required
                  class="flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm ring-offset-background focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
                >
                  {#each credentials as c (c.id)}
                    <option value={c.id}>{c.alias}</option>
                  {/each}
                </select>
              {/if}
              {#if fieldErrors.credential_id}
                <p class="text-sm text-destructive">{fieldErrors.credential_id}</p>
              {/if}
            </div>

            <!-- Submit error -->
            {#if submitError}
              <div role="alert" class="rounded-md border border-destructive/20 bg-destructive/10 p-3 text-sm text-destructive">
                {submitError}
              </div>
            {/if}

            <!-- Submit -->
            <Button
              type="submit"
              class="w-full"
              disabled={submitting || credentials === null || credentials.length === 0}
            >
              {submitting ? 'Creating cluster…' : 'Create cluster'}
            </Button>
          </div>
        </form>
      </CardContent>
    </Card>
  </div>

  <!-- Info panel -->
  <div class="hidden lg:block lg:col-span-2">
    <Card>
      <CardContent class="pt-6">
        <p class="text-sm font-medium text-foreground mb-4">What happens next</p>
        <ul class="space-y-4">
          <li class="flex items-start gap-3">
            <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-muted">
              <Server class="h-4 w-4 text-muted-foreground" />
            </div>
            <p class="text-sm text-muted-foreground leading-relaxed">
              Hetzner servers provisioned in your selected region
            </p>
          </li>
          <li class="flex items-start gap-3">
            <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-muted">
              <Package class="h-4 w-4 text-muted-foreground" />
            </div>
            <p class="text-sm text-muted-foreground leading-relaxed">
              k3s installed and configured automatically
            </p>
          </li>
          <li class="flex items-start gap-3">
            <div class="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-muted">
              <Key class="h-4 w-4 text-muted-foreground" />
            </div>
            <p class="text-sm text-muted-foreground leading-relaxed">
              Kubeconfig ready to download when the cluster is Ready
            </p>
          </li>
        </ul>
      </CardContent>
    </Card>
  </div>
</div>
