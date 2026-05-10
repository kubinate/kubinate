<script lang="ts">
  import { goto } from '$app/navigation';
  import { register } from '$lib/api/auth';

  let email = $state('');
  let displayName = $state('');
  let password = $state('');
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function handleSubmit(e: SubmitEvent) {
    e.preventDefault();
    error = null;
    loading = true;
    try {
      await register(email, displayName, password);
      await goto('/app');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Registration failed. Please try again.';
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>Create account — Kubinate</title></svelte:head>

<div class="relative flex min-h-svh flex-col items-center justify-center bg-zinc-950 px-4 py-12">
  <div
    class="pointer-events-none absolute inset-x-0 top-0 h-96 bg-[radial-gradient(ellipse_80%_60%_at_50%_-10%,rgba(99,102,241,0.12),transparent)]"
    aria-hidden="true"
  ></div>

  <div class="relative z-10 w-full max-w-sm">
    <div class="mb-6 flex flex-col items-center gap-2">
      <div
        class="flex h-10 w-10 items-center justify-center rounded-lg bg-zinc-800 text-lg font-bold text-white"
      >
        K
      </div>
      <span class="text-xl font-bold text-white">Kubinate</span>
    </div>

    <div class="space-y-5 rounded-2xl border border-zinc-800 bg-zinc-900 p-6 shadow-2xl">
      <div class="text-center">
        <h1 class="text-lg font-semibold text-white">Create account</h1>
        <p class="mt-1 text-sm text-zinc-500">to get started with Kubinate</p>
      </div>

      <a
        href="/api/v1/auth/github/start?redirect_to=/app"
        class="flex w-full items-center justify-center gap-2.5 rounded-lg border border-zinc-700 bg-zinc-800 px-4 py-2.5 text-sm font-medium text-white transition hover:bg-zinc-700"
      >
        <svg class="h-4 w-4 shrink-0" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
          <path
            d="M12 .297c-6.63 0-12 5.373-12 12 0 5.303 3.438 9.8 8.205 11.385.6.113.82-.258.82-.577 0-.285-.01-1.04-.015-2.04-3.338.724-4.042-1.61-4.042-1.61C4.422 18.07 3.633 17.7 3.633 17.7c-1.087-.744.084-.729.084-.729 1.205.084 1.838 1.236 1.838 1.236 1.07 1.835 2.809 1.305 3.495.998.108-.776.417-1.305.76-1.605-2.665-.3-5.466-1.332-5.466-5.93 0-1.31.465-2.38 1.235-3.22-.135-.303-.54-1.523.105-3.176 0 0 1.005-.322 3.3 1.23.96-.267 1.98-.399 3-.405 1.02.006 2.04.138 3 .405 2.28-1.552 3.285-1.23 3.285-1.23.645 1.653.24 2.873.12 3.176.765.84 1.23 1.91 1.23 3.22 0 4.61-2.805 5.625-5.475 5.92.42.36.81 1.096.81 2.22 0 1.606-.015 2.896-.015 3.286 0 .315.21.69.825.57C20.565 22.092 24 17.592 24 12.297c0-6.627-5.373-12-12-12"
          />
        </svg>
        Continue with GitHub
      </a>

      <div class="flex items-center gap-3">
        <div class="h-px flex-1 bg-zinc-800"></div>
        <span class="text-xs text-zinc-600">or continue with email</span>
        <div class="h-px flex-1 bg-zinc-800"></div>
      </div>

      <form onsubmit={handleSubmit} class="space-y-3">
        {#if error}
          <div
            class="rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2.5 text-sm text-red-400"
            role="alert"
          >
            {error}
          </div>
        {/if}

        <div>
          <label for="reg-email" class="mb-1.5 block text-xs font-medium text-zinc-400">Email</label
          >
          <input
            id="reg-email"
            type="email"
            bind:value={email}
            required
            autocomplete="email"
            placeholder="you@example.com"
            class="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-white placeholder-zinc-600 outline-none transition focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500/40"
          />
        </div>

        <div>
          <label for="reg-name" class="mb-1.5 block text-xs font-medium text-zinc-400"
            >Display name</label
          >
          <input
            id="reg-name"
            type="text"
            bind:value={displayName}
            required
            autocomplete="name"
            placeholder="Ada Lovelace"
            maxlength={100}
            class="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-white placeholder-zinc-600 outline-none transition focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500/40"
          />
        </div>

        <div>
          <label for="reg-password" class="mb-1.5 block text-xs font-medium text-zinc-400"
            >Password</label
          >
          <input
            id="reg-password"
            type="password"
            bind:value={password}
            required
            autocomplete="new-password"
            placeholder="Min. 8 characters"
            minlength={8}
            class="w-full rounded-lg border border-zinc-700 bg-zinc-800 px-3 py-2 text-sm text-white placeholder-zinc-600 outline-none transition focus:border-indigo-500 focus:ring-1 focus:ring-indigo-500/40"
          />
        </div>

        <button
          type="submit"
          disabled={loading}
          class="w-full rounded-lg bg-indigo-600 px-4 py-2.5 text-sm font-semibold text-white transition hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {loading ? 'Creating account…' : 'Create account'}
        </button>
      </form>
    </div>

    <p class="mt-4 text-center text-sm text-zinc-600">
      Already have an account?
      <a href="/login" class="text-zinc-300 transition hover:text-white">Sign in</a>
    </p>
  </div>
</div>
