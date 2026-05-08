<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError, getBillingState, startCheckout } from '$lib/api/billing';
  import { listMembers } from '$lib/api/team';
  import { getMe } from '$lib/api/me';
  import { externalNavigate } from '$lib/util/navigate';
  import type { BillingPlan, BillingState, MembershipRole } from '$lib/api/schemas';
  import { Button } from '$lib/components/ui/button';
  import { Badge } from '$lib/components/ui/badge';
  import {
    Card,
    CardHeader,
    CardTitle,
    CardDescription,
    CardContent,
    CardFooter
  } from '$lib/components/ui/card';

  let billing = $state<BillingState | null>(null);
  let myRole = $state<MembershipRole | null>(null);
  let loadError = $state<string | null>(null);
  let actionInFlight = $state<BillingPlan | null>(null);
  let actionError = $state<string | null>(null);

  onMount(async () => {
    try {
      const me = await getMe();
      const [state, members] = await Promise.all([
        getBillingState(me.organization_id),
        listMembers(me.organization_id)
      ]);
      billing = state;
      myRole = members.find((m) => m.user_id === me.user_id)?.role ?? null;
    } catch (err) {
      loadError = describe(err);
    }
  });

  function describe(err: unknown): string {
    if (err instanceof ApiError) return `${err.title}: ${err.detail}`;
    if (err instanceof Error) return err.message;
    return 'Unknown error';
  }

  const isOwner = $derived(myRole === 'owner');

  async function upgrade(plan: BillingPlan) {
    actionInFlight = plan;
    actionError = null;
    try {
      const { url } = await startCheckout(plan);
      // Hosted Stripe Checkout — full navigation, not an SPA route.
      externalNavigate(url);
    } catch (err) {
      actionError = describe(err);
    } finally {
      actionInFlight = null;
    }
  }

  function planLabel(plan: BillingPlan): string {
    return plan.charAt(0).toUpperCase() + plan.slice(1);
  }

  // Empty-state copy distinguishes "fresh free org" from
  // "checkout in flight" (Stripe customer linked, plan still free).
  const emptyStateMessage = $derived.by(() => {
    if (!billing) return null;
    if (billing.plan !== 'free') return null;
    if (billing.stripe_customer_id) {
      return 'Your most recent checkout is still being processed by Stripe. Refresh in a minute.';
    }
    return null;
  });
</script>

<svelte:head>
  <title>Billing — Kubinate</title>
</svelte:head>

<h1 class="text-2xl font-semibold mb-6">Billing</h1>

{#if loadError}
  <div
    class="mb-6 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
    role="alert"
  >
    {loadError}
  </div>
{:else if !billing}
  <!-- Loading skeleton -->
  <div class="grid sm:grid-cols-3 gap-4">
    {#each [1, 2, 3] as _ (_)}
      <Card>
        <CardHeader>
          <div class="h-5 w-20 rounded bg-muted animate-pulse mb-1"></div>
          <div class="h-7 w-24 rounded bg-muted animate-pulse"></div>
        </CardHeader>
        <CardContent class="space-y-2">
          {#each [1, 2, 3] as __ (__)}
            <div class="h-4 w-full rounded bg-muted animate-pulse"></div>
          {/each}
        </CardContent>
        <CardFooter>
          <div class="h-9 w-full rounded bg-muted animate-pulse"></div>
        </CardFooter>
      </Card>
    {/each}
  </div>
{:else}
  {#if emptyStateMessage}
    <div
      class="mb-6 rounded-md border border-blue-200 bg-blue-50 dark:border-blue-800 dark:bg-blue-950/40 px-4 py-3 text-sm text-blue-800 dark:text-blue-300"
      role="status"
      data-testid="checkout-pending"
    >
      {emptyStateMessage}
    </div>
  {/if}

  {#if !isOwner}
    <div
      class="mb-6 rounded-md border border-muted bg-muted/40 px-4 py-3 text-sm text-muted-foreground"
      data-testid="non-owner-notice"
    >
      Only owners can change billing. Ask an owner to upgrade.
    </div>
  {/if}

  <div class="grid sm:grid-cols-3 gap-4">
    <!-- Free plan -->
    <Card class={billing.plan === 'free' ? 'ring-2 ring-primary' : ''}>
      <CardHeader>
        <div class="flex items-center justify-between">
          <CardTitle>Free</CardTitle>
          {#if billing.plan === 'free'}
            <Badge variant="secondary">Current plan</Badge>
          {/if}
        </div>
        <CardDescription>
          <span class="text-2xl font-bold text-foreground">$0</span>
          <span class="text-muted-foreground">/mo</span>
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="space-y-2 text-sm text-muted-foreground">
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> 1 cluster
          </li>
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> Community support
          </li>
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> Hetzner CX11 max
          </li>
        </ul>
      </CardContent>
      <CardFooter>
        {#if billing.plan === 'free'}
          <Button variant="outline" class="w-full" disabled>Current plan</Button>
        {:else}
          <Button
            variant="outline"
            class="w-full"
            onclick={() => upgrade('free' as BillingPlan)}
            disabled={!isOwner || actionInFlight !== null}
          >
            Downgrade
          </Button>
        {/if}
      </CardFooter>
    </Card>

    <!-- Starter plan -->
    <Card class={billing.plan === 'starter' ? 'ring-2 ring-primary' : ''}>
      <CardHeader>
        <div class="flex items-center justify-between">
          <CardTitle>Starter</CardTitle>
          {#if billing.plan === 'starter'}
            <Badge variant="secondary">Current plan</Badge>
          {/if}
        </div>
        <CardDescription>
          <span class="text-2xl font-bold text-foreground">$29</span>
          <span class="text-muted-foreground">/mo</span>
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="space-y-2 text-sm text-muted-foreground">
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> 5 clusters
          </li>
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> Email support
          </li>
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> All server types
          </li>
        </ul>
      </CardContent>
      <CardFooter>
        {#if billing.plan === 'starter'}
          <Button class="w-full" disabled>Current plan</Button>
        {:else}
          <Button
            class="w-full"
            onclick={() => upgrade('starter')}
            disabled={!isOwner || actionInFlight !== null}
            data-testid="upgrade-starter"
          >
            {actionInFlight === 'starter' ? 'Redirecting…' : 'Upgrade to Starter'}
          </Button>
        {/if}
      </CardFooter>
    </Card>

    <!-- Pro plan -->
    <Card class={billing.plan === 'pro' ? 'ring-2 ring-primary' : ''}>
      <CardHeader>
        <div class="flex items-center justify-between">
          <CardTitle>Pro</CardTitle>
          {#if billing.plan === 'pro'}
            <Badge variant="secondary">Current plan</Badge>
          {/if}
        </div>
        <CardDescription>
          <span class="text-2xl font-bold text-foreground">$99</span>
          <span class="text-muted-foreground">/mo</span>
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ul class="space-y-2 text-sm text-muted-foreground">
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> Unlimited clusters
          </li>
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> Priority support
          </li>
          <li class="flex items-center gap-2">
            <span class="text-green-500">&#10003;</span> Dedicated account manager
          </li>
        </ul>
      </CardContent>
      <CardFooter>
        {#if billing.plan === 'pro'}
          <Button class="w-full" disabled>Current plan</Button>
        {:else}
          <Button
            class="w-full"
            onclick={() => upgrade('pro')}
            disabled={!isOwner || actionInFlight !== null}
            data-testid="upgrade-pro"
          >
            {actionInFlight === 'pro' ? 'Redirecting…' : 'Upgrade to Pro'}
          </Button>
        {/if}
      </CardFooter>
    </Card>
  </div>

  {#if actionError}
    <div
      class="mt-4 rounded-md border border-destructive/40 bg-destructive/10 px-4 py-3 text-sm text-destructive"
      role="alert"
    >
      {actionError}
    </div>
  {/if}

  <!-- Hidden test id for current-plan -->
  <span class="sr-only" data-testid="current-plan">{planLabel(billing.plan)}</span>
  <span class="sr-only" data-testid="customer-id"
    >{billing.stripe_customer_id ?? 'not linked yet'}</span
  >
{/if}
