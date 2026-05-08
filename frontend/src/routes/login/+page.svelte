<script lang="ts">
  import { page } from '$app/state';

  const error = $derived(page.url.searchParams.get('error'));
</script>

<svelte:head>
  <title>Sign in — Kubinate</title>
</svelte:head>

<div class="flex min-h-svh items-center justify-center bg-background px-4">
  <div class="w-full max-w-sm space-y-6">
    <!-- Wordmark -->
    <div class="space-y-1 text-center">
      <h1 class="text-2xl font-bold tracking-tight">Sign in to Kubinate</h1>
      <p class="text-sm text-muted-foreground">
        Manage k3s clusters on your Hetzner infrastructure
      </p>
    </div>

    <!-- Card -->
    <div class="rounded-xl border border-border bg-card p-6 shadow-sm space-y-4">
      <!-- Error callout -->
      {#if error}
        <div
          class="rounded-md border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive"
          role="alert"
        >
          {error === 'access_denied'
            ? 'GitHub authorisation was denied. Please try again.'
            : 'Sign-in failed. Please try again.'}
        </div>
      {/if}

      <!-- GitHub OAuth link -->
      <a
        href="/api/v1/auth/github/start?redirect_to=/app"
        class="flex w-full items-center justify-center gap-3 rounded-md bg-foreground px-4 py-2.5 text-sm font-semibold text-background transition-colors hover:bg-foreground/90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground"
      >
        <!-- GitHub mark (Invertocat) -->
        <svg
          aria-hidden="true"
          class="size-5 shrink-0"
          viewBox="0 0 24 24"
          fill="currentColor"
          xmlns="http://www.w3.org/2000/svg"
        >
          <path
            d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"
          />
        </svg>
        Continue with GitHub
      </a>

      <!-- Legal footnote -->
      <p class="text-center text-xs text-muted-foreground">
        By signing in, you agree to our
        <a href="/terms" class="underline underline-offset-2 hover:text-foreground">Terms</a>
        and
        <a href="/privacy" class="underline underline-offset-2 hover:text-foreground">Privacy Policy</a>.
      </p>
    </div>
  </div>
</div>
