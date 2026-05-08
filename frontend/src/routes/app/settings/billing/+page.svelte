<script lang="ts">
  import { onMount } from 'svelte';
  import { ApiError, getBillingState, startCheckout } from '$lib/api/billing';
  import { listMembers } from '$lib/api/team';
  import { getMe } from '$lib/api/me';
  import { externalNavigate } from '$lib/util/navigate';
  import type { BillingPlan, BillingState, MembershipRole } from '$lib/api/schemas';

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

<h1>Billing</h1>

{#if loadError}
  <p class="error" role="alert">{loadError}</p>
{:else if !billing}
  <p class="muted">Loading…</p>
{:else}
  <section class="state">
    <dl>
      <dt>Plan</dt>
      <dd data-testid="current-plan">{planLabel(billing.plan)}</dd>
      <dt>Stripe customer</dt>
      <dd data-testid="customer-id">
        {billing.stripe_customer_id ?? 'not linked yet'}
      </dd>
    </dl>
    {#if emptyStateMessage}
      <p class="muted" data-testid="checkout-pending">{emptyStateMessage}</p>
    {/if}
  </section>

  <section class="upgrades">
    <h2>Upgrade</h2>

    {#if !isOwner}
      <p class="muted" data-testid="non-owner-notice">
        Only owners can change billing. Ask an owner to upgrade.
      </p>
    {/if}

    <div class="cta-row">
      <button
        type="button"
        onclick={() => upgrade('starter')}
        disabled={!isOwner || actionInFlight !== null || billing.plan === 'starter'}
        data-testid="upgrade-starter"
      >
        {actionInFlight === 'starter'
          ? 'Redirecting…'
          : billing.plan === 'starter'
            ? 'Current plan'
            : 'Upgrade to Starter'}
      </button>
      <button
        type="button"
        onclick={() => upgrade('pro')}
        disabled={!isOwner || actionInFlight !== null || billing.plan === 'pro'}
        data-testid="upgrade-pro"
      >
        {actionInFlight === 'pro'
          ? 'Redirecting…'
          : billing.plan === 'pro'
            ? 'Current plan'
            : 'Upgrade to Pro'}
      </button>
    </div>

    {#if actionError}
      <p class="error" role="alert">{actionError}</p>
    {/if}
  </section>
{/if}

<style>
  h1 {
    font-size: 1.8rem;
    margin-bottom: 1rem;
  }
  h2 {
    font-size: 1.1rem;
    margin: 1.5rem 0 0.5rem;
  }
  section {
    max-width: 640px;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.5rem 1.5rem;
  }
  dt {
    font-weight: 600;
    color: #333;
  }
  .cta-row {
    display: flex;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
  .cta-row button {
    padding: 0.6rem 1.2rem;
    background: #1a7f37;
    color: #fff;
    border: 0;
    border-radius: 4px;
    font-weight: 600;
    cursor: pointer;
  }
  .cta-row button[disabled] {
    background: #888;
    cursor: not-allowed;
  }
  .muted {
    color: #555;
  }
  .error {
    color: #b00020;
  }
</style>
