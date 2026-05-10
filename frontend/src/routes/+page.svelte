<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Server,
    Shield,
    Terminal,
    Puzzle,
    FileText,
    Users,
    ChevronDown,
    CheckCircle2
  } from 'lucide-svelte';

  onMount(() => {
    const targets = document.querySelectorAll('.reveal-target');
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add('revealed');
            observer.unobserve(entry.target);
          }
        }
      },
      { threshold: 0.1 }
    );
    for (const el of targets) observer.observe(el);
    return () => observer.disconnect();
  });
</script>

<svelte:head>
  <title>Kubinate — Managed k3s on infrastructure you own</title>
  <meta
    name="description"
    content="Kubinate provisions production-ready k3s clusters on your Hetzner account. Zero ops overhead. Full kubectl access."
  />
</svelte:head>

<!-- ===== Nav ===== -->
<nav class="sticky top-0 z-50 border-b border-zinc-800/50 bg-zinc-950/80 backdrop-blur-md">
  <div class="mx-auto flex max-w-6xl items-center justify-between px-6 py-3">
    <span class="text-lg font-bold text-white">Kubinate</span>
    <div class="flex items-center gap-4">
      <a href="/login" class="text-sm text-zinc-400 transition-colors hover:text-white">
        Sign in
      </a>
      <a
        href="/login"
        class="rounded-full bg-white px-4 py-1.5 text-sm font-medium text-black transition-colors hover:bg-zinc-100"
      >
        Get started
      </a>
    </div>
  </div>
</nav>

<!-- ===== Hero ===== -->
<section
  class="relative flex min-h-screen flex-col items-center justify-center bg-zinc-950 px-6 text-center"
>
  <!-- Radial glow -->
  <div
    class="pointer-events-none absolute inset-x-0 top-0 h-full bg-[radial-gradient(ellipse_80%_50%_at_50%_-20%,rgba(99,102,241,0.15),transparent)]"
    aria-hidden="true"
  ></div>

  <div class="relative z-10 flex flex-col items-center">
    <!-- Badge -->
    <span
      class="fade-up mb-6 inline-block rounded-full border border-indigo-500/30 bg-indigo-500/10 px-3 py-1 text-xs font-medium text-indigo-300"
      style="animation-delay: 0s"
    >
      Managed k3s · Now in private beta
    </span>

    <!-- Headline -->
    <h1
      class="fade-up max-w-4xl text-5xl font-bold leading-[1.1] tracking-tight text-white sm:text-6xl lg:text-7xl"
      style="animation-delay: 0.1s"
    >
      k3s clusters on infrastructure you own.
    </h1>

    <!-- Sub -->
    <p class="fade-up mt-4 max-w-xl text-lg text-zinc-400" style="animation-delay: 0.2s">
      Kubinate provisions production-ready k3s clusters on your Hetzner account. Zero ops overhead.
      Full kubectl access.
    </p>

    <!-- CTAs -->
    <div
      class="fade-up mt-8 flex flex-wrap items-center justify-center gap-3"
      style="animation-delay: 0.3s"
    >
      <a
        href="/login"
        class="rounded-lg bg-white px-5 py-2.5 text-sm font-semibold text-black transition hover:bg-zinc-100"
      >
        Get started free
      </a>
      <a
        href="/docs"
        class="rounded-lg border border-zinc-700 px-5 py-2.5 text-sm font-semibold text-zinc-300 transition hover:border-zinc-500 hover:text-white"
      >
        Read the docs
      </a>
    </div>
  </div>

  <!-- Scroll chevron -->
  <div class="absolute bottom-10 left-1/2 -translate-x-1/2 animate-bounce text-zinc-600">
    <ChevronDown class="h-6 w-6" />
  </div>
</section>

<!-- ===== Stats bar ===== -->
<div class="border-y border-zinc-800/60 bg-zinc-900/40 py-8">
  <div class="flex flex-wrap items-center justify-center gap-16 px-6 sm:gap-24">
    <div class="text-center">
      <p class="text-3xl font-bold text-white">&lt; 5 min</p>
      <p class="mt-0.5 text-sm text-zinc-500">to first cluster</p>
    </div>
    <div class="text-center">
      <p class="text-3xl font-bold text-white">100%</p>
      <p class="mt-0.5 text-sm text-zinc-500">kubectl compatible</p>
    </div>
    <div class="text-center">
      <p class="text-3xl font-bold text-white">Your Hetzner</p>
      <p class="mt-0.5 text-sm text-zinc-500">infrastructure</p>
    </div>
  </div>
</div>

<!-- ===== How it works ===== -->
<section class="py-24">
  <div class="mx-auto max-w-6xl px-6">
    <p class="mb-3 text-xs font-semibold uppercase tracking-widest text-indigo-400">How it works</p>
    <h2 class="mb-16 text-3xl font-bold text-white">From zero to running cluster in minutes</h2>

    <div class="grid grid-cols-1 gap-8 md:grid-cols-3">
      <!-- Step 1 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/60 p-6 transition-colors hover:border-zinc-700"
      >
        <p class="mb-4 text-4xl font-bold text-zinc-700">01</p>
        <h3 class="mb-2 text-base font-semibold text-white">Connect</h3>
        <p class="text-sm leading-relaxed text-zinc-400">
          Plug in your Hetzner API key. Kubinate provisions servers in your account — you stay in
          control of billing and data residency.
        </p>
      </div>

      <!-- Step 2 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/60 p-6 transition-colors hover:border-zinc-700"
        style="transition-delay: 0.1s"
      >
        <p class="mb-4 text-4xl font-bold text-zinc-700">02</p>
        <h3 class="mb-2 text-base font-semibold text-white">Create</h3>
        <p class="text-sm leading-relaxed text-zinc-400">
          Pick a region, node type, and worker count. Kubinate bootstraps k3s, configures
          networking, and issues a kubeconfig in under 5 minutes.
        </p>
      </div>

      <!-- Step 3 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/60 p-6 transition-colors hover:border-zinc-700"
        style="transition-delay: 0.2s"
      >
        <p class="mb-4 text-4xl font-bold text-zinc-700">03</p>
        <h3 class="mb-2 text-base font-semibold text-white">Deploy</h3>
        <p class="text-sm leading-relaxed text-zinc-400">
          Use standard <code class="text-zinc-300">kubectl</code>, Helm, or any CI/CD pipeline. Add
          ingress, cert-manager, or observability with a single click from the add-ons catalogue.
        </p>
      </div>
    </div>
  </div>
</section>

<!-- ===== Features grid ===== -->
<section class="py-24">
  <div class="mx-auto max-w-6xl px-6">
    <p class="mb-3 text-xs font-semibold uppercase tracking-widest text-indigo-400">
      Everything you need
    </p>
    <h2 class="mb-12 text-3xl font-bold text-white">Production-ready from day one</h2>

    <div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
      <!-- Card 1 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/40 p-5 transition-colors hover:bg-zinc-900/80"
      >
        <Server class="mb-3 h-5 w-5 text-indigo-400" />
        <h3 class="mb-1 text-sm font-semibold text-white">Your infrastructure</h3>
        <p class="text-sm leading-relaxed text-zinc-500">
          Clusters run on Hetzner servers in your own account. No multi-tenant pools.
        </p>
      </div>

      <!-- Card 2 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/40 p-5 transition-colors hover:bg-zinc-900/80"
        style="transition-delay: 0.05s"
      >
        <Shield class="mb-3 h-5 w-5 text-indigo-400" />
        <h3 class="mb-1 text-sm font-semibold text-white">Automatic TLS</h3>
        <p class="text-sm leading-relaxed text-zinc-500">
          cert-manager pre-installed. Ingress NGINX ready for HTTPS from the start.
        </p>
      </div>

      <!-- Card 3 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/40 p-5 transition-colors hover:bg-zinc-900/80"
        style="transition-delay: 0.1s"
      >
        <Terminal class="mb-3 h-5 w-5 text-indigo-400" />
        <h3 class="mb-1 text-sm font-semibold text-white">Full kubectl access</h3>
        <p class="text-sm leading-relaxed text-zinc-500">
          Standard kubeconfig. No proprietary CLI required.
        </p>
      </div>

      <!-- Card 4 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/40 p-5 transition-colors hover:bg-zinc-900/80"
        style="transition-delay: 0.15s"
      >
        <Puzzle class="mb-3 h-5 w-5 text-indigo-400" />
        <h3 class="mb-1 text-sm font-semibold text-white">Add-on catalogue</h3>
        <p class="text-sm leading-relaxed text-zinc-500">
          Install ingress, observability, and more with a single click.
        </p>
      </div>

      <!-- Card 5 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/40 p-5 transition-colors hover:bg-zinc-900/80"
        style="transition-delay: 0.2s"
      >
        <FileText class="mb-3 h-5 w-5 text-indigo-400" />
        <h3 class="mb-1 text-sm font-semibold text-white">Audit log</h3>
        <p class="text-sm leading-relaxed text-zinc-500">
          Every action by every team member logged and queryable.
        </p>
      </div>

      <!-- Card 6 -->
      <div
        class="reveal-target rounded-xl border border-zinc-800 bg-zinc-900/40 p-5 transition-colors hover:bg-zinc-900/80"
        style="transition-delay: 0.25s"
      >
        <Users class="mb-3 h-5 w-5 text-indigo-400" />
        <h3 class="mb-1 text-sm font-semibold text-white">Team access</h3>
        <p class="text-sm leading-relaxed text-zinc-500">
          Invite team members with Owner or Member roles. SSO coming soon.
        </p>
      </div>
    </div>
  </div>
</section>

<!-- ===== Terminal preview ===== -->
<section class="py-24">
  <div class="mx-auto max-w-6xl px-6">
    <div class="grid grid-cols-1 items-center gap-16 lg:grid-cols-2">
      <!-- Left: text -->
      <div class="reveal-target">
        <p class="mb-3 text-xs font-semibold uppercase tracking-widest text-indigo-400">
          Developer experience
        </p>
        <h2 class="mb-8 text-3xl font-bold text-white">Feels like local. Runs in the cloud.</h2>
        <ul class="space-y-4">
          <li class="flex items-start gap-3">
            <CheckCircle2 class="mt-0.5 h-5 w-5 shrink-0 text-emerald-400" />
            <span class="text-sm leading-relaxed text-zinc-400">
              Kubeconfig delivered on cluster ready
            </span>
          </li>
          <li class="flex items-start gap-3">
            <CheckCircle2 class="mt-0.5 h-5 w-5 shrink-0 text-emerald-400" />
            <span class="text-sm leading-relaxed text-zinc-400">
              Works with any kubectl plugin or Helm chart
            </span>
          </li>
          <li class="flex items-start gap-3">
            <CheckCircle2 class="mt-0.5 h-5 w-5 shrink-0 text-emerald-400" />
            <span class="text-sm leading-relaxed text-zinc-400">
              Agent-initiated mTLS — no inbound ports to open
            </span>
          </li>
        </ul>
      </div>

      <!-- Right: terminal mockup -->
      <div class="reveal-target" style="transition-delay: 0.15s">
        <div class="rounded-xl border border-zinc-800 bg-zinc-950 p-5 font-mono text-sm">
          <!-- Window chrome dots -->
          <div class="mb-4 flex items-center gap-1.5">
            <span class="h-3 w-3 rounded-full bg-zinc-700"></span>
            <span class="h-3 w-3 rounded-full bg-zinc-700"></span>
            <span class="h-3 w-3 rounded-full bg-zinc-700"></span>
          </div>

          <p>
            <span class="text-zinc-500">$</span>
            <span class="text-white"> kubinate cluster create </span>
            <span class="text-indigo-300">my-prod</span>
            <span class="text-zinc-500"> --region </span>
            <span class="text-indigo-300">nbg1</span>
          </p>

          <div class="mt-4 space-y-1.5">
            <p>
              <span class="text-zinc-500">✓</span>
              <span class="text-zinc-300"> Provisioning 3 nodes in nbg1...</span>
            </p>
            <p>
              <span class="text-zinc-500">✓</span>
              <span class="text-zinc-300"> k3s installed and joined</span>
            </p>
            <p>
              <span class="text-zinc-500">✓</span>
              <span class="text-zinc-300"> Kubeconfig written to ~/.kube/config</span>
            </p>
          </div>

          <p class="mt-4">
            <span class="text-emerald-400">Cluster my-prod is ready</span>
            <span class="text-zinc-500"> (4m 32s)</span>
            <span class="cursor-blink ml-0.5 text-zinc-300">|</span>
          </p>
        </div>
      </div>
    </div>
  </div>
</section>

<!-- ===== CTA banner ===== -->
<section class="border-t border-zinc-800/60 py-20 px-6">
  <div class="mx-auto max-w-2xl text-center">
    <h2 class="text-3xl font-bold text-white">Deploy your first cluster today.</h2>
    <p class="mt-3 text-base text-zinc-400">No credit card required for the private beta.</p>
    <a
      href="/login"
      class="mt-6 inline-block rounded-full bg-white px-6 py-2.5 text-sm font-semibold text-black transition-colors hover:bg-zinc-100"
    >
      Get started free
    </a>
  </div>
</section>

<!-- ===== Footer ===== -->
<footer class="border-t border-zinc-800/60 py-8 px-6">
  <div class="mx-auto flex max-w-6xl items-center justify-between">
    <p class="text-sm text-zinc-500">
      <span class="font-semibold text-zinc-400">Kubinate</span> &copy; 2026
    </p>
    <div class="flex items-center gap-6">
      <a href="/status" class="text-sm text-zinc-500 transition-colors hover:text-zinc-300"
        >Status</a
      >
      <a href="/docs" class="text-sm text-zinc-500 transition-colors hover:text-zinc-300">Docs</a>
      <a href="/privacy" class="text-sm text-zinc-500 transition-colors hover:text-zinc-300"
        >Privacy</a
      >
    </div>
  </div>
</footer>

<style>
  @keyframes fade-up {
    from {
      opacity: 0;
      transform: translateY(20px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .fade-up {
    animation: fade-up 0.6s ease-out both;
  }

  @keyframes cursor-blink {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0;
    }
  }

  .cursor-blink {
    animation: cursor-blink 1s step-end infinite;
  }

  .reveal-target {
    opacity: 0;
    transform: translateY(16px);
    transition:
      opacity 0.6s ease-out,
      transform 0.6s ease-out;
  }

  :global(.reveal-target.revealed) {
    opacity: 1;
    transform: translateY(0);
  }
</style>
