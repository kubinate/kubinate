<svelte:head>
  <title>Kubinate — Managed k3s on infrastructure you own</title>
  <meta
    name="description"
    content="Kubinate provisions production-ready k3s clusters on your Hetzner account. You pay Hetzner for the hardware. We handle the rest."
  />
</svelte:head>

<!-- Hero -->
<section class="bg-foreground text-background flex min-h-screen flex-col items-center justify-center px-6 py-24 text-center">
  <h1 class="max-w-3xl text-4xl font-bold leading-tight tracking-tight sm:text-5xl lg:text-6xl">
    Managed k3s on infrastructure you own.
  </h1>
  <p class="mt-6 max-w-xl text-lg leading-relaxed opacity-80">
    Kubinate provisions production-ready k3s clusters on your Hetzner account. You pay Hetzner for
    the hardware. We handle the rest.
  </p>
  <div class="mt-10 flex flex-wrap items-center justify-center gap-4">
    <a
      href="/login"
      class="bg-background text-foreground hover:bg-background/90 rounded-lg px-6 py-3 text-sm font-semibold transition-colors"
    >
      Start free
    </a>
    <a
      href="/docs"
      class="border-background/40 text-background hover:bg-background/10 rounded-lg border px-6 py-3 text-sm font-semibold transition-colors"
    >
      Read the docs
    </a>
  </div>
</section>

<!-- Feature grid -->
<section class="bg-background px-6 py-20">
  <div class="mx-auto max-w-5xl">
    <div class="grid gap-8 sm:grid-cols-3">
      <!-- Card: Your infrastructure -->
      <div class="rounded-xl border border-border p-8">
        <div class="mb-4 flex h-10 w-10 items-center justify-center rounded-lg bg-muted">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            class="text-foreground"
            aria-hidden="true"
          >
            <!-- Server icon -->
            <rect width="20" height="8" x="2" y="2" rx="2" ry="2"></rect>
            <rect width="20" height="8" x="2" y="14" rx="2" ry="2"></rect>
            <line x1="6" x2="6.01" y1="6" y2="6"></line>
            <line x1="6" x2="6.01" y1="18" y2="18"></line>
          </svg>
        </div>
        <h3 class="text-foreground text-base font-semibold">Your infrastructure</h3>
        <p class="text-muted-foreground mt-2 text-sm leading-relaxed">
          Clusters run on Hetzner servers in your own account. No multi-tenant compute pools — your
          workloads stay isolated on hardware you control.
        </p>
      </div>

      <!-- Card: Production-ready -->
      <div class="rounded-xl border border-border p-8">
        <div class="mb-4 flex h-10 w-10 items-center justify-center rounded-lg bg-muted">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            class="text-foreground"
            aria-hidden="true"
          >
            <!-- Shield icon -->
            <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"></path>
          </svg>
        </div>
        <h3 class="text-foreground text-base font-semibold">Production-ready</h3>
        <p class="text-muted-foreground mt-2 text-sm leading-relaxed">
          Every cluster ships with automatic TLS, RBAC, add-on management, and built-in
          observability so you can focus on shipping software.
        </p>
      </div>

      <!-- Card: Full control -->
      <div class="rounded-xl border border-border p-8">
        <div class="mb-4 flex h-10 w-10 items-center justify-center rounded-lg bg-muted">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            class="text-foreground"
            aria-hidden="true"
          >
            <!-- Terminal icon -->
            <polyline points="4 17 10 11 4 5"></polyline>
            <line x1="12" x2="20" y1="19" y2="19"></line>
          </svg>
        </div>
        <h3 class="text-foreground text-base font-semibold">Full control</h3>
        <p class="text-muted-foreground mt-2 text-sm leading-relaxed">
          Access your clusters with standard kubectl. Bring your own Helm charts, CRDs, and
          operators. No vendor lock-in, ever.
        </p>
      </div>
    </div>
  </div>
</section>

<!-- Footer -->
<footer class="bg-background border-t border-border px-6 py-8 text-center">
  <p class="text-muted-foreground text-sm">© 2026 Kubinate</p>
</footer>
