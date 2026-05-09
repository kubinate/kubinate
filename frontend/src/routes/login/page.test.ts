/**
 * Login page — static page with GitHub OAuth link and error handling.
 *
 * Three specs:
 *   1. Renders the GitHub CTA link with the correct href and text.
 *   2. Shows access_denied copy when ?error=access_denied is in the URL.
 *   3. Shows generic error copy when ?error=some_other_error is in the URL.
 *
 * No API calls; the page is fully static. Only $app/state is mocked to
 * control the URL search params.
 */

import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => {
  const params = { current: new URLSearchParams() };
  return { params };
});

vi.mock('$app/state', () => ({
  get page() {
    return { url: { searchParams: mocks.params.current } as URL, params: {} };
  }
}));

import LoginPage from './+page.svelte';

beforeEach(() => {
  mocks.params.current = new URLSearchParams();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe('login page', () => {
  it('renders the GitHub CTA link with the correct href and label', () => {
    const { getByRole } = render(LoginPage);

    const link = getByRole('link', { name: /continue with github/i });
    expect(link).toBeInTheDocument();
    expect(link.getAttribute('href')).toBe('/api/v1/auth/github/start?redirect_to=/app');
  });

  it('shows access_denied copy when ?error=access_denied is present', () => {
    mocks.params.current = new URLSearchParams('error=access_denied');

    const { getByRole } = render(LoginPage);

    const alert = getByRole('alert');
    expect(alert).toBeInTheDocument();
    expect(alert.textContent ?? '').toContain('GitHub authorisation was denied');
  });

  it('shows generic error copy when ?error=some_other_error is present', () => {
    mocks.params.current = new URLSearchParams('error=some_other_error');

    const { getByRole } = render(LoginPage);

    const alert = getByRole('alert');
    expect(alert).toBeInTheDocument();
    expect(alert.textContent ?? '').toContain('Sign-in failed');
  });
});
