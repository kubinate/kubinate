<script lang="ts">
  let { children } = $props();
</script>

<header>
  <a href="/">Kubinate</a>
  <nav>
    <a href="/pricing">Pricing</a>
    <a href="/docs">Docs</a>
    <a href="/app">Dashboard</a>
  </nav>
</header>

<main>
  {@render children()}
</main>

<footer>
  <p>© Kubinate — self-service k3s on Hetzner.</p>
</footer>

<style>
  header,
  main,
  footer {
    max-width: 72rem;
    margin: 0 auto;
    padding: 1rem;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  nav a {
    margin-left: 1rem;
  }
</style>
