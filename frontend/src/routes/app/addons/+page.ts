import type { PageLoad } from './$types';

export const load: PageLoad = async ({ fetch }) => {
  // fetch catalog for addon slugs; fall back to hardcoded list on error
  try {
    const res = await fetch('/api/v1/catalog/clusters');
    if (res.ok) {
      const catalog = await res.json();
      return { addonSlugs: catalog.addons as string[] };
    }
  } catch {
    /* ignore */
  }
  return { addonSlugs: ['ingress-nginx', 'cert-manager'] };
};
