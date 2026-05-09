/**
 * Sprint 5 — settings/integrations page coverage.
 *
 * Five flows under test:
 *   1. Empty state when no credentials exist.
 *   2. Credential list renders rows with correct alias text.
 *   3. Add success — POST issued, new row appears.
 *   4. Add validation — empty alias shows inline error, no fetch call.
 *   5. Add API error — 500 response surfaces error alert in dialog.
 *   6. Delete success — DELETE issued, row disappears.
 *
 * `createCredential` / `deleteCredential` use the global `fetch` directly,
 * so tests stub `global.fetch` via `vi.stubGlobal`.
 *
 * shadcn Dialog renders via a portal outside the test container, so dialog
 * content is found on `document.body` rather than inside `container`.
 */

import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import IntegrationsPage from './+page.svelte';

// ── fetch helpers ─────────────────────────────────────────────────────────────

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

// ── fixtures ──────────────────────────────────────────────────────────────────

const CRED_A = {
  id: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
  alias: 'staging',
  created_at: '2026-05-01T00:00:00Z',
  updated_at: '2026-05-01T00:00:00Z'
};

const CRED_B = {
  id: 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb',
  alias: 'production',
  created_at: '2026-05-02T00:00:00Z',
  updated_at: '2026-05-02T00:00:00Z'
};

const CRED_NEW = {
  id: 'cccccccc-cccc-cccc-cccc-cccccccccccc',
  alias: 'new-token',
  created_at: '2026-05-09T00:00:00Z',
  updated_at: '2026-05-09T00:00:00Z'
};

// ── lifecycle ─────────────────────────────────────────────────────────────────

beforeEach(() => {
  vi.unstubAllGlobals();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

// ── tests ─────────────────────────────────────────────────────────────────────

describe('settings/integrations — empty state', () => {
  it('renders "No tokens yet." when data.credentials is empty', () => {
    const { container } = render(IntegrationsPage, {
      props: { data: { credentials: [] } }
    });

    expect(container.textContent ?? '').toContain('No tokens yet.');
  });
});

describe('settings/integrations — credential list', () => {
  it('renders rows for each credential with correct alias text', async () => {
    const { findByTestId, getByTestId } = render(IntegrationsPage, {
      props: { data: { credentials: [CRED_A, CRED_B] } }
    });

    const rowA = await findByTestId(`credential-row-${CRED_A.id}`);
    expect(rowA.textContent ?? '').toContain('staging');

    const rowB = getByTestId(`credential-row-${CRED_B.id}`);
    expect(rowB.textContent ?? '').toContain('production');
  });
});

describe('settings/integrations — add credential', () => {
  it('success — POST issued and new credential row appears', async () => {
    const fetchStub = installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      if (url.endsWith('/api/v1/integrations/hetzner') && method === 'POST') {
        return fakeResponse(CRED_NEW, 201);
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(IntegrationsPage, {
      props: { data: { credentials: [] } }
    });

    // Open the add modal.
    await fireEvent.click(await findByTestId('add-token-button'));

    // Dialog is portal-rendered; query document.body for inputs.
    await waitFor(() => {
      expect(document.body.textContent ?? '').toContain('Add Hetzner token');
    });

    const aliasInput = document.body.querySelector(
      '[data-testid="add-alias-input"]'
    ) as HTMLInputElement;
    const tokenInput = document.body.querySelector(
      '[data-testid="add-token-input"]'
    ) as HTMLInputElement;
    expect(aliasInput).not.toBeNull();
    expect(tokenInput).not.toBeNull();

    // Fill in alias and token. Svelte 5's bind:value reads event.target.value
    // on both 'input' and 'change' events. We set the property directly first
    // then dispatch an 'input' event so the reactive binding updates.
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set?.call(
      aliasInput,
      'new-token'
    );
    aliasInput.dispatchEvent(new Event('input', { bubbles: true }));
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set?.call(
      tokenInput,
      'hv1_supersecret'
    );
    tokenInput.dispatchEvent(new Event('input', { bubbles: true }));

    const saveButton = document.body.querySelector(
      '[data-testid="add-token-confirm"]'
    ) as HTMLElement;
    await fireEvent.click(saveButton);

    await waitFor(() => {
      expect(
        fetchStub.mock.calls.some(([input, init]) => {
          const url = typeof input === 'string' ? input : input.toString();
          return (
            url.endsWith('/api/v1/integrations/hetzner') &&
            (init?.method ?? '').toUpperCase() === 'POST'
          );
        })
      ).toBe(true);
    });

    // The new credential row should appear.
    await findByTestId(`credential-row-${CRED_NEW.id}`);
  });

  it('validation — empty alias shows error without calling fetch', async () => {
    const fetchStub = installFetch(async () => fakeResponse({}, 200));

    const { findByTestId } = render(IntegrationsPage, {
      props: { data: { credentials: [] } }
    });

    await fireEvent.click(await findByTestId('add-token-button'));

    await waitFor(() => {
      expect(document.body.textContent ?? '').toContain('Add Hetzner token');
    });

    // Leave alias empty — click Save immediately.
    const saveButton = document.body.querySelector(
      '[data-testid="add-token-confirm"]'
    ) as HTMLElement;
    await fireEvent.click(saveButton);

    await waitFor(() => {
      expect(document.body.textContent ?? '').toContain('Alias is required');
    });

    // fetch must not have been called.
    expect(fetchStub).not.toHaveBeenCalled();
  });

  it('API error — 500 response shows error alert in the dialog', async () => {
    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      if (url.endsWith('/api/v1/integrations/hetzner') && method === 'POST') {
        return fakeResponse(
          {
            type: 'about:blank',
            title: 'Internal Server Error',
            status: 500,
            detail: 'create credential failed'
          },
          500
        );
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(IntegrationsPage, {
      props: { data: { credentials: [] } }
    });

    await fireEvent.click(await findByTestId('add-token-button'));

    await waitFor(() => {
      expect(document.body.textContent ?? '').toContain('Add Hetzner token');
    });

    const aliasInput = document.body.querySelector(
      '[data-testid="add-alias-input"]'
    ) as HTMLInputElement;
    const tokenInput = document.body.querySelector(
      '[data-testid="add-token-input"]'
    ) as HTMLInputElement;

    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set?.call(
      aliasInput,
      'staging'
    );
    aliasInput.dispatchEvent(new Event('input', { bubbles: true }));
    Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set?.call(
      tokenInput,
      'hv1_bad'
    );
    tokenInput.dispatchEvent(new Event('input', { bubbles: true }));

    const saveButton = document.body.querySelector(
      '[data-testid="add-token-confirm"]'
    ) as HTMLElement;
    await fireEvent.click(saveButton);

    await waitFor(() => {
      const alerts = document.body.querySelectorAll('[role="alert"]');
      const found = Array.from(alerts).some((el) =>
        (el.textContent ?? '').includes('create credential failed')
      );
      expect(found).toBe(true);
    });
  });
});

describe('settings/integrations — delete credential', () => {
  it('success — DELETE issued and row disappears', async () => {
    const fetchStub = installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      if (url.includes('/api/v1/integrations/hetzner/') && method === 'DELETE') {
        return new Response(null, { status: 204 });
      }
      return fakeResponse({}, 404);
    });

    const { findByTestId, queryByTestId } = render(IntegrationsPage, {
      props: { data: { credentials: [CRED_A] } }
    });

    // Verify the row is present initially.
    expect(await findByTestId(`credential-row-${CRED_A.id}`)).toBeInTheDocument();

    // Click the Remove button for CRED_A.
    await fireEvent.click(await findByTestId(`remove-${CRED_A.id}`));

    // Delete confirmation dialog appears in the portal.
    await waitFor(() => {
      expect(document.body.textContent ?? '').toContain('Remove token');
    });

    const confirmButton = document.body.querySelector(
      '[data-testid="delete-confirm"]'
    ) as HTMLElement;
    expect(confirmButton).not.toBeNull();
    await fireEvent.click(confirmButton);

    await waitFor(() => {
      expect(
        fetchStub.mock.calls.some(([input, init]) => {
          const url = typeof input === 'string' ? input : input.toString();
          return (
            url.includes(`/api/v1/integrations/hetzner/${CRED_A.id}`) &&
            (init?.method ?? '').toUpperCase() === 'DELETE'
          );
        })
      ).toBe(true);
    });

    // The credential row must no longer be in the DOM.
    await waitFor(() => {
      expect(queryByTestId(`credential-row-${CRED_A.id}`)).toBeNull();
    });
  });
});
