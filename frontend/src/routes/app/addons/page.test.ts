/**
 * Add-ons page — prop-driven static rendering.
 *
 * Three specs:
 *   1. Empty state renders "No add-ons available." when addonSlugs is [].
 *   2. Known slugs (ingress-nginx, cert-manager) render their ADDON_META
 *      display names in the card grid.
 *   3. An unknown slug falls back to the slug itself as the card title.
 *
 * No API calls and no mocking needed — data is passed in as a prop.
 */

import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import AddonsPage from './+page.svelte';

describe('addons page', () => {
  it('shows empty state when addonSlugs is empty', () => {
    const { getByText } = render(AddonsPage, {
      props: { data: { addonSlugs: [] } }
    });

    expect(getByText('No add-ons available.')).toBeInTheDocument();
  });

  it('renders known addon cards with display names from ADDON_META', () => {
    const { getByText, getAllByText } = render(AddonsPage, {
      props: { data: { addonSlugs: ['ingress-nginx', 'cert-manager'] } }
    });

    // 'Ingress NGINX' only appears as the card title — one match expected.
    expect(getByText('Ingress NGINX')).toBeInTheDocument();
    // 'cert-manager' appears as both the CardTitle and the namespace <code>
    // element, so we use getAllByText and assert at least one match.
    expect(getAllByText('cert-manager').length).toBeGreaterThanOrEqual(1);
  });

  it('falls back to the slug as the card title for unknown slugs', () => {
    const { getByText } = render(AddonsPage, {
      props: { data: { addonSlugs: ['my-custom-addon'] } }
    });

    expect(getByText('my-custom-addon')).toBeInTheDocument();
  });
});
