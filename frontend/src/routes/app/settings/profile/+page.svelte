<script lang="ts">
  import { page } from '$app/state';
  import { onMount } from 'svelte';
  import { updateMe } from '$lib/api/me';
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

  let displayName = $state('');
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let savedFlash = $state(false);

  onMount(() => {
    displayName = page.data.session.displayName ?? '';
  });

  async function doSave() {
    saving = true;
    saveError = null;
    try {
      await updateMe(displayName.trim());
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
  <title>Profile — Kubinate</title>
</svelte:head>

<h1 class="text-2xl font-semibold tracking-tight mb-6">Profile</h1>

<div class="grid gap-6 md:grid-cols-2">
  <!-- Card 1: Account info (read-only) -->
  <Card>
    <CardHeader>
      <CardTitle>Account</CardTitle>
      <CardDescription>Read-only details about your account.</CardDescription>
    </CardHeader>
    <CardContent>
      <div class="space-y-3">
        <div class="flex flex-col gap-0.5">
          <span class="text-xs text-muted-foreground uppercase tracking-wide">User ID</span>
          <span class="font-mono text-sm truncate" title={page.data.session.userId}>
            {page.data.session.userId}
          </span>
        </div>
        <div class="flex flex-col gap-0.5">
          <span class="text-xs text-muted-foreground uppercase tracking-wide">Email</span>
          <span class="font-mono text-sm truncate" title={page.data.session.email}>
            {page.data.session.email}
          </span>
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
          placeholder="Your name"
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
        disabled={saving ||
          displayName.trim() === (page.data.session.displayName ?? '') ||
          displayName.trim().length === 0}
      >
        {saving ? 'Saving…' : 'Save'}
      </Button>
    </CardContent>
  </Card>
</div>
