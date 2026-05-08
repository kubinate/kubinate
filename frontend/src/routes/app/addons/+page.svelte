<script lang="ts">
  import { Card, CardHeader, CardTitle, CardContent } from '$lib/components/ui/card';

  interface AddonMeta {
    name: string;
    description: string;
    namespace: string;
    chartRepo: string;
    docsUrl?: string;
  }

  const ADDON_META: Record<string, AddonMeta> = {
    'ingress-nginx': {
      name: 'Ingress NGINX',
      description: 'HTTP/HTTPS load balancer and ingress controller for your cluster.',
      namespace: 'ingress-nginx',
      chartRepo: 'kubernetes.github.io/ingress-nginx'
    },
    'cert-manager': {
      name: 'cert-manager',
      description:
        "Automated TLS certificate management via Let's Encrypt and other ACME providers.",
      namespace: 'cert-manager',
      chartRepo: 'charts.jetstack.io'
    }
  };

  function metaFor(slug: string): AddonMeta {
    return (
      ADDON_META[slug] ?? {
        name: slug,
        description: '',
        namespace: '',
        chartRepo: ''
      }
    );
  }

  let { data } = $props<{ data: { addonSlugs: string[] } }>();
</script>

<svelte:head>
  <title>Add-ons — Kubinate</title>
</svelte:head>

<div class="flex flex-col gap-6">
  <div>
    <h1 class="text-2xl font-semibold">Add-ons</h1>
    <p class="mt-1 text-sm text-muted-foreground">
      Extend your clusters with pre-configured Helm charts.
    </p>
  </div>

  {#if data.addonSlugs.length === 0}
    <div class="flex flex-1 items-center justify-center py-24">
      <p class="text-sm text-muted-foreground">No add-ons available.</p>
    </div>
  {:else}
    <div class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
      {#each data.addonSlugs as slug (slug)}
        {@const meta = metaFor(slug)}
        <Card>
          <CardHeader>
            <CardTitle>{meta.name}</CardTitle>
          </CardHeader>
          <CardContent class="flex flex-col gap-4">
            {#if meta.description}
              <p class="text-sm text-muted-foreground">{meta.description}</p>
            {/if}

            <dl class="space-y-1.5 text-sm">
              {#if meta.namespace}
                <div class="flex flex-wrap items-baseline gap-x-2">
                  <dt class="font-medium text-foreground shrink-0">Namespace</dt>
                  <dd>
                    <code class="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">
                      {meta.namespace}
                    </code>
                  </dd>
                </div>
              {/if}
              {#if meta.chartRepo}
                <div class="flex flex-wrap items-baseline gap-x-2">
                  <dt class="font-medium text-foreground shrink-0">Chart repo</dt>
                  <dd>
                    <code class="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">
                      {meta.chartRepo}
                    </code>
                  </dd>
                </div>
              {/if}
            </dl>

            <p class="text-xs text-muted-foreground">
              Install from the
              <a
                href="/app"
                class="underline underline-offset-2 hover:text-foreground transition-colors"
              >
                cluster detail page
              </a>.
            </p>
          </CardContent>
        </Card>
      {/each}
    </div>
  {/if}
</div>
