<script lang="ts">
  import type { MfaState } from '$lib/api/schemas';

  interface Props {
    mfaState: MfaState | null;
  }

  let { mfaState }: Props = $props();
</script>

{#if mfaState === 'must_enrol'}
  <div data-testid="mfa-enrol-banner" role="alert" class="banner">
    Set up a passkey — your role requires MFA.
    <a href="/app/settings/security">Configure now →</a>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.75rem 1rem;
    background-color: #fefce8;
    border: 1px solid #fde047;
    border-radius: 6px;
    color: #713f12;
  }

  .banner a {
    color: #92400e;
    font-weight: 600;
    white-space: nowrap;
  }
</style>
