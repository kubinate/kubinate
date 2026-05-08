<script lang="ts">
  let { children, data } = $props();
</script>

<aside>
  <p>Signed in as {data.session.userId}</p>
  <nav>
    <a href="/app">Clusters</a>
    <a href="/app/addons">Add-ons</a>
    <a href="/app/settings">Settings</a>
  </nav>
</aside>

<section>
  {@render children()}
</section>

<style>
  aside {
    padding: 1rem;
    background: #f7f7f7;
    border-radius: 6px;
  }
  section {
    padding: 1rem;
  }
</style>
