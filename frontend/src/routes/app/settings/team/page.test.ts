/**
 * Sprint 2 ticket 06 frontend follow-up — Vitest specs covering the
 * two mutating happy paths called out in the DoD: role change and
 * member removal.
 */

import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import TeamPage from './+page.svelte';

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

function fakeNoContent(): Response {
  return new Response(null, { status: 204 });
}

// Pre-allocated membership row UUIDs so the Zod schema (which insists
// `id` is a UUID) parses cleanly. `crypto.randomUUID()` would also work
// but pinning makes failure messages stable.
const MEMBERSHIP_IDS: Record<string, string> = {
  [ALICE]: 'cccccccc-cccc-cccc-cccc-cccccccccccc',
  [BOB]: 'dddddddd-dddd-dddd-dddd-dddddddddddd'
};

function membershipRow(user_id: string, name: string, role: string) {
  return {
    id: MEMBERSHIP_IDS[user_id] ?? '00000000-0000-0000-0000-000000000000',
    user_id,
    email: `${name}@example.test`,
    display_name: name,
    role,
    joined_at: '2026-04-26T10:00:00Z',
    last_active_at: '2026-04-26T11:00:00Z'
  };
}

interface FetchHandler {
  (url: string, init?: FetchInit): Response | Promise<Response>;
}

function installFetch(handler: FetchHandler) {
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

describe('team settings page', () => {
  it('changes a member role and refreshes the list', async () => {
    let bobRole = 'developer';
    const calls: { method: string; url: string; body?: string }[] = [];

    const stub = installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      const body = typeof init?.body === 'string' ? init.body : init?.body?.toString();
      calls.push({ method, url, body });

      if (url.endsWith('/v1/me')) {
        return fakeResponse({
          user_id: ALICE,
          session_id: '99999999-9999-9999-9999-999999999999',
          organization_id: ORG
        });
      }
      if (url.endsWith(`/v1/organizations/${ORG}/members`)) {
        return fakeResponse([
          membershipRow(ALICE, 'alice', 'owner'),
          membershipRow(BOB, 'bob', bobRole)
        ]);
      }
      if (url.endsWith(`/v1/organizations/${ORG}/invites`)) {
        return fakeResponse([]);
      }
      if (method === 'PATCH' && url.endsWith(`/v1/organizations/${ORG}/members/${BOB}`)) {
        bobRole = JSON.parse(body ?? '{}').role;
        return fakeNoContent();
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(TeamPage);

    const roleSelect = (await findByTestId(`role-select-${BOB}`)) as HTMLSelectElement;
    expect(roleSelect.value).toBe('developer');

    await fireEvent.change(roleSelect, { target: { value: 'admin' } });

    // The PATCH must fire, then the page should re-fetch members and
    // re-render with the new role.
    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.method === 'PATCH' &&
            c.url.endsWith(`/v1/organizations/${ORG}/members/${BOB}`) &&
            c.body?.includes('admin')
        )
      ).toBe(true);
    });

    await waitFor(async () => {
      const updated = (await findByTestId(`role-select-${BOB}`)) as HTMLSelectElement;
      expect(updated.value).toBe('admin');
    });

    expect(stub).toHaveBeenCalled();
  });

  it('removes a member and the row disappears', async () => {
    let bobPresent = true;
    const calls: { method: string; url: string }[] = [];

    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      calls.push({ method, url });

      if (url.endsWith('/v1/me')) {
        return fakeResponse({
          user_id: ALICE,
          session_id: '99999999-9999-9999-9999-999999999999',
          organization_id: ORG
        });
      }
      if (url.endsWith(`/v1/organizations/${ORG}/members`)) {
        const rows = [membershipRow(ALICE, 'alice', 'owner')];
        if (bobPresent) rows.push(membershipRow(BOB, 'bob', 'developer'));
        return fakeResponse(rows);
      }
      if (url.endsWith(`/v1/organizations/${ORG}/invites`)) {
        return fakeResponse([]);
      }
      if (method === 'DELETE' && url.endsWith(`/v1/organizations/${ORG}/members/${BOB}`)) {
        bobPresent = false;
        return fakeNoContent();
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId, queryByTestId } = render(TeamPage);

    const removeBtn = await findByTestId(`remove-${BOB}`);
    await fireEvent.click(removeBtn);

    await waitFor(() => {
      expect(
        calls.some(
          (c) => c.method === 'DELETE' && c.url.endsWith(`/v1/organizations/${ORG}/members/${BOB}`)
        )
      ).toBe(true);
    });

    await waitFor(() => {
      expect(queryByTestId(`member-row-${BOB}`)).toBeNull();
    });
  });
});
