/**
 * Sprint 3 ticket 04 — billing settings page.
 *
 * Two DoD specs:
 *   * Owner sees enabled upgrade buttons; non-owner sees disabled.
 *   * Clicking "Upgrade to Starter" issues the POST and navigates to
 *     the returned URL (mocked).
 */

import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Capture every navigation the component triggers. `vi.mock` factories
// hoist above the import, so we route through a hoisted closure.
const { navigatedTo } = vi.hoisted(() => ({ navigatedTo: [] as string[] }));
vi.mock('$lib/util/navigate', () => ({
  externalNavigate: (url: string) => {
    navigatedTo.push(url);
  }
}));

import BillingPage from './+page.svelte';

const ORG = '11111111-1111-1111-1111-111111111111';
const ALICE = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';
const BOB = 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb';

type FetchInit = Parameters<typeof fetch>[1];
type FetchInput = Parameters<typeof fetch>[0];

function fakeResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' }
  });
}

function membershipRow(user_id: string, name: string, role: string) {
  const ids: Record<string, string> = {
    [ALICE]: 'cccccccc-cccc-cccc-cccc-cccccccccccc',
    [BOB]: 'dddddddd-dddd-dddd-dddd-dddddddddddd'
  };
  return {
    id: ids[user_id] ?? '00000000-0000-0000-0000-000000000000',
    user_id,
    email: `${name}@example.test`,
    display_name: name,
    role,
    joined_at: '2026-04-30T10:00:00Z',
    last_active_at: null
  };
}

function meEnvelope(user_id: string) {
  return {
    user_id,
    session_id: '99999999-9999-9999-9999-999999999999',
    organization_id: ORG
  };
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
  navigatedTo.length = 0;
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('billing settings page', () => {
  it('non-owner sees disabled upgrade buttons + a notice', async () => {
    installFetch(async (url) => {
      if (url.endsWith('/v1/me')) return fakeResponse(meEnvelope(BOB));
      if (url.endsWith(`/v1/organizations/${ORG}/billing`))
        return fakeResponse({ plan: 'free', stripe_customer_id: null });
      if (url.endsWith(`/v1/organizations/${ORG}/members`))
        return fakeResponse([
          membershipRow(ALICE, 'alice', 'owner'),
          membershipRow(BOB, 'bob', 'developer')
        ]);
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(BillingPage);

    const notice = await findByTestId('non-owner-notice');
    expect(notice).toBeInTheDocument();

    const starter = (await findByTestId('upgrade-starter')) as HTMLButtonElement;
    const pro = (await findByTestId('upgrade-pro')) as HTMLButtonElement;
    expect(starter.disabled).toBe(true);
    expect(pro.disabled).toBe(true);
  });

  it('owner click on Upgrade Starter posts checkout + navigates to the hosted URL', async () => {
    const calls: { method: string; url: string; body?: string }[] = [];

    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      const body = typeof init?.body === 'string' ? init.body : init?.body?.toString();
      calls.push({ method, url, body });

      if (url.endsWith('/v1/me')) return fakeResponse(meEnvelope(ALICE));
      if (url.endsWith(`/v1/organizations/${ORG}/billing`))
        return fakeResponse({ plan: 'free', stripe_customer_id: null });
      if (url.endsWith(`/v1/organizations/${ORG}/members`))
        return fakeResponse([membershipRow(ALICE, 'alice', 'owner')]);
      if (method === 'POST' && url.endsWith('/v1/billing/checkout')) {
        return fakeResponse({ url: 'https://checkout.stripe.com/c/pay/test_session' });
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(BillingPage);

    const starter = (await findByTestId('upgrade-starter')) as HTMLButtonElement;
    expect(starter.disabled).toBe(false);

    await fireEvent.click(starter);

    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.method === 'POST' &&
            c.url.endsWith('/v1/billing/checkout') &&
            c.body?.includes('starter')
        )
      ).toBe(true);
    });
    await waitFor(() => {
      expect(navigatedTo.at(-1)).toBe('https://checkout.stripe.com/c/pay/test_session');
    });
  });
});
