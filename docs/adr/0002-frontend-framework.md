# ADR-0002: Frontend framework

- **Status**: Accepted
- **Date**: 2026-04-24
- **Deciders**: Founding team
- **Tags**: frontend, framework

## Context

Kubinate's web surface has three distinct workloads:

1. **Marketing / landing pages**: public, SEO-sensitive, mostly static.
2. **Authenticated dashboard**: SPA-like, data-dense, real-time updates
   for cluster status and provisioning progress.
3. **Embedded observability views**: Grafana panels wrapped in our own
   chrome and auth proxy.

These need not all be the same app, but consolidating them reduces
context-switching for a small team. The relevant frameworks are React
(with Next.js or Remix), SvelteKit, SolidStart, and a minority view of
server-rendered HTML + HTMX.

## Decision

We will use **SvelteKit** as the sole frontend framework. It is deployed
to **Cloudflare Pages** using the `@sveltejs/adapter-cloudflare` adapter
with edge functions for SSR of public routes. Dashboard routes are
client-rendered (CSR) after authentication. Typed API access uses an
OpenAPI-generated client against the Rust backend's spec.

## Alternatives considered

- **Next.js (React)**: largest ecosystem, most hireable. Rejected because
  the bundle size and runtime overhead are larger than SvelteKit with no
  observed benefit for our feature set, and because React's rendering
  mental model adds complexity (memoization, re-render gotchas) that
  SvelteKit sidesteps via compiled reactivity.
- **Remix**: loved by this team, but the Cloudflare Pages story is
  weaker and we lose the compiled-reactivity advantage.
- **SolidStart**: technically compelling but less mature and with a
  markedly smaller ecosystem; we would bet the frontend on a small
  maintainer group.
- **HTMX + server-rendered HTML**: simpler, but the real-time update
  story for long-running cluster provisioning is worse, and embedding a
  Grafana-like experience without significant client-side state is
  awkward.

## Consequences

**We accept:**

- Smaller component ecosystem than React. We will likely build some UI
  primitives ourselves (tables, command palette, drawer) rather than
  pulling them from an ecosystem.
- Hiring: fewer candidates list Svelte as their primary stack. Our
  mitigation is that Svelte has a short learning curve for anyone who
  has used React, Vue, or Angular; we do not require prior Svelte
  experience.
- Some third-party SDKs (observability widgets, payment providers) ship
  React components only. We either wrap them behind a component wrapper
  or fall back to their vanilla JS SDK.

**We assume (bets):**

- SvelteKit's adapter ecosystem (specifically Cloudflare Pages with SSR)
  remains production-ready. We verify this in a Phase 0 spike.
- The compiled-reactivity and smaller bundle continue to yield a
  dashboard that feels faster than the React alternative.

**Positive follow-ons:**

- Cloudflare Pages gives us free CDN, DDoS protection, and atomic
  deploys with preview URLs per PR.
- SvelteKit server routes are usable as a Backend-for-Frontend (BFF)
  layer if we want to avoid exposing some API details to the browser.

## Revisit trigger

- A must-have third-party integration ships only a React SDK with no
  reasonable wrapper path and we cannot ship without it, **or**
- Cloudflare Pages adapter for SvelteKit regresses in a way that forces
  a move off Pages, at which point the whole stack is re-evaluated.

## References

- [SvelteKit documentation](https://kit.svelte.dev)
- [Cloudflare Pages + SvelteKit guide](https://developers.cloudflare.com/pages/framework-guides/deploy-a-svelte-site/)
- Phase 0 spike: "SvelteKit SSR on Cloudflare Pages — PoC".
