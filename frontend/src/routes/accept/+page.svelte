<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { acceptInvite, ApiError } from '$lib/api/team';
  import { Button } from '$lib/components/ui/button';
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent
  } from '$lib/components/ui/card';
  import { Loader2, AlertCircle } from 'lucide-svelte';
  import type { PageData } from './$types';

  const { data }: { data: PageData } = $props();

  type Phase = 'pending' | 'error';

  let phase: Phase = $state('pending');
  let errorTitle = $state('');
  let errorDetail = $state('');

  async function redeem() {
    phase = 'pending';
    errorTitle = '';
    errorDetail = '';
    try {
      await acceptInvite(data.token);
      goto('/app');
    } catch (err) {
      if (err instanceof ApiError) {
        errorTitle = err.title;
        errorDetail = err.detail;
      } else {
        errorTitle = 'Unexpected error';
        errorDetail = 'Something went wrong. Please try again.';
      }
      phase = 'error';
    }
  }

  onMount(() => {
    redeem();
  });
</script>

<svelte:head>
  <title>Join your team — Kubinate</title>
</svelte:head>

<div class="flex min-h-svh items-center justify-center bg-background px-4">
  <div class="w-full max-w-sm space-y-6">
    <div class="space-y-1 text-center">
      <h1 class="text-2xl font-bold tracking-tight">Join your team</h1>
      <p class="text-sm text-muted-foreground">
        Kubinate — managed k3s on your Hetzner infrastructure
      </p>
    </div>

    <Card>
      <CardHeader class="items-center text-center">
        {#if phase === 'pending'}
          <Loader2 class="size-8 animate-spin text-muted-foreground" />
          <CardTitle>Accepting invite…</CardTitle>
          <CardDescription>Hang tight while we add you to the organisation.</CardDescription>
        {:else}
          <AlertCircle class="size-8 text-destructive" />
          <CardTitle>Invite failed</CardTitle>
          <CardDescription>{errorTitle}</CardDescription>
        {/if}
      </CardHeader>

      {#if phase === 'error'}
        <CardContent class="space-y-4">
          <div
            class="rounded-md border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive"
            role="alert"
          >
            {errorDetail}
          </div>

          <div class="flex flex-col gap-2">
            <Button onclick={redeem}>Try again</Button>
            <Button variant="ghost" href="/login">Back to login</Button>
          </div>
        </CardContent>
      {/if}
    </Card>
  </div>
</div>
