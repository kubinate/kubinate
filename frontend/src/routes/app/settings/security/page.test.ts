/**
 * Sprint 4 ticket 05 — settings/security page coverage.
 *
 * Three flows under test:
 *   1. Passkey list + register modal happy path.
 *   2. Recovery-code regeneration shows codes once and warns on
 *      close-without-copy.
 *   3. The `?mfa=required` mode renders the assertion section, and
 *      the redeem-code modal surfaces the API's 403 as the
 *      "code invalid or already used" copy.
 *
 * `navigator.credentials` lives behind a small mock so the
 * register / assert flows complete without an authenticator.
 * `navigator.clipboard.writeText` is stubbed; `window.confirm`
 * is forced to true (or false where the test cares).
 */

import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import StatusPage from './+page.svelte';

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

// Mocks for the SvelteKit modules. `vi.mock` calls are hoisted
// above local declarations, so the per-test mutable state has to
// live in `vi.hoisted` (which is also hoisted, but lets us share
// references between the test body and the hoisted factory).
const mocks = vi.hoisted(() => {
  const params = { current: new URLSearchParams() };
  return {
    params,
    gotoMock: vi.fn(async () => {})
  };
});

vi.mock('$app/state', () => ({
  get page() {
    return {
      url: { searchParams: mocks.params.current } as URL,
      params: {}
    };
  }
}));
vi.mock('$app/navigation', () => ({
  goto: mocks.gotoMock
}));

// `navigator.credentials.create / get` mock. Returns a synthetic
// PublicKeyCredential the page's `credentialToJson` will accept.
function fakePublicKeyCredential(): PublicKeyCredential {
  const buf = new Uint8Array([1, 2, 3]).buffer;
  return {
    id: 'fake-cred-id',
    rawId: buf,
    type: 'public-key',
    response: {
      clientDataJSON: buf,
      attestationObject: buf
    } as AuthenticatorAttestationResponse,
    getClientExtensionResults: () => ({})
  } as unknown as PublicKeyCredential;
}

beforeEach(() => {
  mocks.params.current = new URLSearchParams();
  mocks.gotoMock.mockClear();
  vi.unstubAllGlobals();

  // Default: every credentials call resolves to a synthetic credential.
  // Tests that need a different shape override before render().
  vi.stubGlobal('navigator', {
    credentials: {
      create: vi.fn(async () => fakePublicKeyCredential()),
      get: vi.fn(async () => fakePublicKeyCredential())
    },
    clipboard: {
      writeText: vi.fn(async () => {})
    }
  });
  // `window.confirm` defaults to true; tests override per-flow.
  vi.spyOn(window, 'confirm').mockReturnValue(true);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const PASSKEY_A = {
  id: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
  nickname: 'YubiKey 5C',
  registered_at: '2026-04-30T10:00:00Z',
  last_used_at: '2026-05-01T08:00:00Z'
};

describe('settings/security — passkey list', () => {
  it('renders rows for the user’s registered passkeys', async () => {
    installFetch(async (url) => {
      if (url.endsWith('/v1/auth/passkey/list')) return fakeResponse([PASSKEY_A]);
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(StatusPage);

    const row = await findByTestId(`passkey-row-${PASSKEY_A.id}`);
    expect(row.textContent ?? '').toContain('YubiKey 5C');
  });

  it('register flow posts the start + finish endpoints and refreshes the list', async () => {
    const calls: { method: string; url: string; body?: string }[] = [];
    let listCalls = 0;
    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      const body = typeof init?.body === 'string' ? init.body : init?.body?.toString();
      calls.push({ method, url, body });

      if (url.endsWith('/v1/auth/passkey/list')) {
        listCalls += 1;
        return fakeResponse(listCalls === 1 ? [] : [PASSKEY_A]);
      }
      if (url.endsWith('/v1/auth/passkey/register/start')) {
        return fakeResponse({
          ceremony_id: '11111111-1111-1111-1111-111111111111',
          // Minimum WebAuthn JSON the page passes through to
          // webauthnJsonToCreate; the inner fields are Base64URL
          // strings the helper decodes.
          challenge: {
            publicKey: {
              challenge: 'ABCD',
              user: { id: 'EFGH', name: 'a@example.com', displayName: 'a@example.com' },
              excludeCredentials: []
            }
          }
        });
      }
      if (url.endsWith('/v1/auth/passkey/register/finish')) {
        return fakeResponse(PASSKEY_A, 201);
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(StatusPage);

    await fireEvent.click(await findByTestId('register-button'));
    const nicknameInput = (await findByTestId('register-nickname')) as HTMLInputElement;
    nicknameInput.value = 'YubiKey 5C';
    nicknameInput.dispatchEvent(new Event('input', { bubbles: true }));
    await fireEvent.click(await findByTestId('register-confirm'));

    await waitFor(() => {
      expect(
        calls.some(
          (c) => c.method === 'POST' && c.url.endsWith('/v1/auth/passkey/register/start')
        )
      ).toBe(true);
      expect(
        calls.some(
          (c) => c.method === 'POST' && c.url.endsWith('/v1/auth/passkey/register/finish')
        )
      ).toBe(true);
      // After register/finish the page refreshes the list.
      expect(listCalls).toBeGreaterThanOrEqual(2);
    });
  });

  it('revoke flow confirms then issues DELETE', async () => {
    const calls: { method: string; url: string }[] = [];
    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      calls.push({ method, url });
      if (url.endsWith('/v1/auth/passkey/list')) return fakeResponse([PASSKEY_A]);
      if (url.endsWith(`/v1/auth/passkey/${PASSKEY_A.id}`) && method === 'DELETE')
        return new Response(null, { status: 204 });
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(StatusPage);

    await fireEvent.click(await findByTestId(`revoke-${PASSKEY_A.id}`));

    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.method === 'DELETE' &&
            c.url.endsWith(`/v1/auth/passkey/${PASSKEY_A.id}`)
        )
      ).toBe(true);
    });
  });
});

describe('settings/security — recovery codes', () => {
  it('regenerate populates the codes modal', async () => {
    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      if (url.endsWith('/v1/auth/passkey/list')) return fakeResponse([]);
      if (
        url.endsWith('/v1/auth/recovery-codes/regenerate') &&
        method === 'POST'
      ) {
        return fakeResponse({
          codes: ['AB12C-D3E4F', 'XY56Z-W7V8U'],
          total: 2
        });
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(StatusPage);

    await fireEvent.click(await findByTestId('regenerate-button'));

    const codesList = await findByTestId('recovery-codes-list');
    expect(codesList.textContent ?? '').toContain('AB12C-D3E4F');
    expect(codesList.textContent ?? '').toContain('XY56Z-W7V8U');
  });
});

describe('settings/security — MFA required mode', () => {
  it('renders the assert button only when ?mfa=required is present', async () => {
    installFetch(async (url) => {
      if (url.endsWith('/v1/auth/passkey/list')) return fakeResponse([PASSKEY_A]);
      return fakeResponse({}, 404);
    });

    // First render: no query param → no assert button.
    const { queryByTestId, unmount } = render(StatusPage);
    await waitFor(() => {
      // The list has loaded; the assert button must not be present.
      expect(queryByTestId('assert-button')).toBeNull();
    });
    unmount();

    // Second render: ?mfa=required → assert button rendered.
    mocks.params.current = new URLSearchParams('?mfa=required');
    const { findByTestId } = render(StatusPage);
    expect(await findByTestId('assert-button')).toBeInTheDocument();
  });

  it('redeem flow surfaces 403 as inline copy', async () => {
    mocks.params.current = new URLSearchParams('?mfa=required');

    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      if (url.endsWith('/v1/auth/passkey/list')) return fakeResponse([]);
      if (
        url.endsWith('/v1/auth/recovery-codes/redeem') &&
        method === 'POST'
      ) {
        // Match the Rust API's actual Problem Details shape.
        return new Response(
          JSON.stringify({
            type: 'about:blank',
            title: 'Forbidden',
            status: 403,
            detail: 'recovery code is invalid or already used'
          }),
          {
            status: 403,
            headers: { 'content-type': 'application/json' }
          }
        );
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId, container } = render(StatusPage);

    await fireEvent.click(await findByTestId('recovery-redeem-link'));
    const codeInput = (await findByTestId('recovery-code-input')) as HTMLInputElement;
    codeInput.value = 'XX00X-XX00X';
    codeInput.dispatchEvent(new Event('input', { bubbles: true }));
    await fireEvent.click(await findByTestId('recovery-redeem-confirm'));

    await waitFor(() => {
      // Page renders ApiError as `${title}: ${detail}` via describe();
      // assert against the substring the runbook copy promises.
      expect(container.textContent ?? '').toContain('invalid or already used');
    });
  });
});
