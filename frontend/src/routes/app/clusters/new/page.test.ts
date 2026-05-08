/**
 * Sprint 2 ticket 04 DoD test — verifies that the credential picker
 * renders correctly for both the "no credentials yet" and the
 * "populated list" cases. We mount the actual page component with a
 * stubbed `fetch`; no testing-library magic beyond what's needed.
 */

import { render, fireEvent, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import NewClusterPage from './+page.svelte';

type FetchInit = Parameters<typeof fetch>[1];
type FetchInput = Parameters<typeof fetch>[0];

function fakeResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' }
  });
}

function installFetch(handler: (url: string, init?: FetchInit) => Response | Promise<Response>) {
  const stub = vi.fn(async (input: FetchInput, init?: FetchInit) => {
    const url = typeof input === 'string' ? input : input.toString();
    return handler(url, init);
  });
  vi.stubGlobal('fetch', stub);
  return stub;
}

beforeEach(() => {
  vi.unstubAllGlobals();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('cluster create form — credential picker', () => {
  it('renders the empty-state link when no credentials exist', async () => {
    installFetch((url) => {
      if (url.endsWith('/v1/catalog/clusters')) {
        return fakeResponse({
          regions: ['nbg1', 'fsn1'],
          server_types: ['cpx21']
        });
      }
      if (url.endsWith('/v1/integrations/hetzner')) {
        return fakeResponse([]);
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId, queryByTestId } = render(NewClusterPage);

    const empty = await findByTestId('no-credentials');
    expect(empty).toBeInTheDocument();
    expect(empty.textContent ?? '').toMatch(/add one/i);
    // Picker must not render when the list is empty.
    expect(queryByTestId('credential-picker')).toBeNull();
  });

  it('renders the picker and pre-selects the first credential', async () => {
    const credentials = [
      {
        id: '11111111-1111-1111-1111-111111111111',
        alias: 'staging',
        created_at: '2026-04-26T00:00:00Z',
        updated_at: '2026-04-26T00:00:00Z'
      },
      {
        id: '22222222-2222-2222-2222-222222222222',
        alias: 'production',
        created_at: '2026-04-26T00:00:00Z',
        updated_at: '2026-04-26T00:00:00Z'
      }
    ];

    installFetch((url) => {
      if (url.endsWith('/v1/catalog/clusters')) {
        return fakeResponse({
          regions: ['nbg1'],
          server_types: ['cpx21']
        });
      }
      if (url.endsWith('/v1/integrations/hetzner')) {
        return fakeResponse(credentials);
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(NewClusterPage);

    const picker = (await findByTestId('credential-picker')) as HTMLSelectElement;
    expect(picker).toBeInTheDocument();
    // First credential pre-selected so a one-credential org doesn't
    // need an extra click.
    expect(picker.value).toBe(credentials[0].id);

    // Switching the selection updates the bound value.
    await fireEvent.change(picker, { target: { value: credentials[1].id } });
    await waitFor(() => expect(picker.value).toBe(credentials[1].id));
  });
});
