/**
 * Landing page — static marketing page with no data prop and no API calls.
 *
 * Four specs:
 *   1. The hero <h1> contains the product tagline.
 *   2. The "Start free" CTA links to /login.
 *   3. The "Read the docs" CTA links to /docs.
 *   4. All three feature card headings are rendered.
 */

import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import LandingPage from './+page.svelte';

describe('landing page', () => {
  it('renders the hero headline', () => {
    const { getByRole } = render(LandingPage);

    const heading = getByRole('heading', { level: 1 });
    expect(heading).toBeInTheDocument();
    expect(heading.textContent).toContain('Managed k3s on infrastructure you own.');
  });

  it('renders the "Start free" CTA linking to /login', () => {
    const { getByRole } = render(LandingPage);

    const link = getByRole('link', { name: /start free/i });
    expect(link).toBeInTheDocument();
    expect(link.getAttribute('href')).toBe('/login');
  });

  it('renders the "Read the docs" CTA linking to /docs', () => {
    const { getByRole } = render(LandingPage);

    const link = getByRole('link', { name: /read the docs/i });
    expect(link).toBeInTheDocument();
    expect(link.getAttribute('href')).toBe('/docs');
  });

  it('renders all three feature card headings', () => {
    const { getByText } = render(LandingPage);

    expect(getByText('Your infrastructure')).toBeInTheDocument();
    expect(getByText('Production-ready')).toBeInTheDocument();
    expect(getByText('Full control')).toBeInTheDocument();
  });
});
