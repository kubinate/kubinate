<script lang="ts">
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import { getOrganization, updateOrganization } from '$lib/api/organization';
  import type { OrgView } from '$lib/api/schemas';
  import { Button } from '$lib/components/ui/button';
  import { Card, CardHeader, CardTitle, CardDescription, CardContent } from '$lib/components/ui/card';
  import { Input } from '$lib/components/ui/input';
  import { Label } from '$lib/components/ui/label';

  let org = $state<OrgView | null>(null);
  let loadError = $state<string | null>(null);
  let displayName = $state('');
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let savedFlash = $state(false);

  onMount(async () => {
    const orgId: string = page.data.session.organizationId;
    try {
      const loaded = await getOrganization(orgId);
      org = loaded;
      displayName = loaded.display_name;
    } catch (err) {
      loadError = err instanceof Error ? err.message : 'Failed to load organization';
    }
  });

  async function doSave() {
    if (!org) return;
    const orgId: string = page.data.session.organizationId;
    saving = true;
    saveError = null;
    try {
      const updated = await updateOrganization(orgId, displayName);
      org = updated;
      savedFlash = true;
      setTimeout(() => {
        savedFlash = false;
      }, 2000);
    } catch (err) {
      saveError = err instanceof Error ? err.message : 'Save failed';
    } finally {
      saving = false;
    }
  }
</script>

<svelte:head>
  <title>General — Kubinate</title>
</svelte:head>

<h1 class="text-2xl font-semibold tracking-tight mb-6">General</h1>

{#if loadError}
  <div
    class="mb-4 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
    role="alert"
  >
    {loadError}
  </div>
{/if}

{#if org === null && !loadError}
  <p class="text-sm text-muted-foreground">Loading…</p>
{:else if org !== null}
  <div class="grid gap-6 md:grid-cols-2">
    <!-- Card 1: Organization info (read-only) -->
    <Card>
      <CardHeader>
        <CardTitle>Organization</CardTitle>
        <CardDescription>Read-only details about your organization.</CardDescription>
      </CardHeader>
      <CardContent>
        <div class="space-y-3">
          <div class="flex flex-col gap-0.5">
            <span class="text-xs text-muted-foreground uppercase tracking-wide">ID</span>
            <span class="font-mono text-sm truncate" title={org.id}>{org.id}</span>
          </div>
          <div class="flex flex-col gap-0.5">
            <span class="text-xs text-muted-foreground uppercase tracking-wide">Slug</span>
            <span class="font-mono text-sm truncate" title={org.slug}>{org.slug}</span>
          </div>
          <div class="flex flex-col gap-0.5">
            <span class="text-xs text-muted-foreground uppercase tracking-wide">Created</span>
            <span class="text-sm">{new Date(org.created_at).toLocaleDateString()}</span>
          </div>
        </div>
      </CardContent>
    </Card>

    <!-- Card 2: Display name (editable) -->
    <Card>
      <CardHeader>
        <CardTitle>Display name</CardTitle>
        <CardDescription>The name shown across the dashboard.</CardDescription>
      </CardHeader>
      <CardContent class="space-y-4">
        <div class="space-y-1.5">
          <Label for="display-name">Display name</Label>
          <Input
            id="display-name"
            type="text"
            bind:value={displayName}
            disabled={saving}
            placeholder="My Organization"
          />
          {#if saveError}
            <p class="text-destructive text-sm mt-1">{saveError}</p>
          {/if}
          {#if savedFlash}
            <p class="text-sm text-green-600 mt-1">Saved!</p>
          {/if}
        </div>
        <Button
          onclick={doSave}
          disabled={saving || displayName === org.display_name || displayName.trim().length === 0}
        >
          {saving ? 'Saving…' : 'Save'}
        </Button>
      </CardContent>
    </Card>
  </div>
{/if}
