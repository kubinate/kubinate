/**
 * Sprint 3 ticket 03 — addon panel + install modal coverage.
 * Sprint 3 ticket 10 — SSE happy path + reconnect / fallback coverage.
 *
 * The cluster status page itself (polling, error mapping) is exercised
 * indirectly here; these specs focus on the AC's two rows: an
 * "Install ingress-nginx" CTA appears for a Ready cluster with no
 * addons, and a `failed` row renders the sanitised `status_reason`
 * (never raw Helm error text).
 */

import { fireEvent, render, waitFor } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import StatusPage from './+page.svelte';

// Minimal EventSource fake. jsdom doesn't ship one, and we want
// programmatic control over `connected`/`step`/`terminal` dispatches +
// `onerror` to drive the reconnect path.
// The `Partial<EventSource>` shape was caught by stricter
// addEventListener overloads after Sprint 4 ticket 05's other
// type churn. We don't need to satisfy the full EventSource
// interface — the `vi.stubGlobal('EventSource', FakeEventSource as
// unknown as typeof EventSource)` cast at the call site is what
// matters at runtime.
class FakeEventSource {
  static instances: FakeEventSource[] = [];
  url: string;
  listeners = new Map<string, Set<(evt: MessageEvent) => void>>();
  onerror: ((this: EventSource, ev: Event) => unknown) | null = null;
  closed = false;
  constructor(url: string) {
    this.url = url;
    FakeEventSource.instances.push(this);
  }
  addEventListener(type: string, fn: (evt: MessageEvent) => void) {
    const s = this.listeners.get(type) ?? new Set();
    s.add(fn);
    this.listeners.set(type, s);
  }
  close() {
    this.closed = true;
  }
  emit(type: string, data: unknown = {}) {
    const evt = new MessageEvent(type, { data: JSON.stringify(data) });
    for (const fn of this.listeners.get(type) ?? []) fn(evt);
  }
  triggerError() {
    this.onerror?.call(this as unknown as EventSource, new Event('error'));
  }
  static reset() {
    FakeEventSource.instances = [];
  }
}

// vi.mock factories are hoisted above any local `const`, so the
// cluster id has to live inside the factory body or via vi.hoisted().
const CLUSTER_ID = '11111111-1111-1111-1111-111111111111';

vi.mock('$app/state', () => ({
  page: { params: { id: '11111111-1111-1111-1111-111111111111' } }
}));
vi.mock('$app/navigation', () => ({
  goto: vi.fn(async () => {})
}));

type FetchInit = Parameters<typeof fetch>[1];
type FetchInput = Parameters<typeof fetch>[0];

function fakeResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' }
  });
}

function readyCluster(): Record<string, unknown> {
  return {
    id: CLUSTER_ID,
    name: 'acme-prod',
    region: 'nbg1',
    server_type: 'cpx21',
    control_plane_count: 1,
    worker_count: 1,
    status: 'ready',
    current_step: null,
    started_at: '2026-04-30T10:00:00Z',
    error_category: null,
    terminal: true,
    kubeconfig_available: true,
    created_at: '2026-04-30T10:00:00Z',
    updated_at: '2026-04-30T10:01:00Z'
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
  FakeEventSource.reset();
  vi.stubGlobal('EventSource', FakeEventSource as unknown as typeof EventSource);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  FakeEventSource.reset();
});

describe('cluster status page — addon panel', () => {
  it('renders the install CTA for a Ready cluster with no addons', async () => {
    installFetch(async (url) => {
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}`)) return fakeResponse(readyCluster());
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}/addons`)) return fakeResponse([]);
      if (url.endsWith('/v1/catalog/clusters'))
        return fakeResponse({
          regions: ['nbg1'],
          server_types: ['cpx21'],
          addons: ['ingress-nginx', 'cert-manager']
        });
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(StatusPage);

    const cta = await findByTestId('install-ingress-nginx');
    expect(cta).toBeInTheDocument();
    expect(cta.textContent ?? '').toMatch(/install ingress-nginx/i);
  });

  it('issues the install POST when the modal is confirmed', async () => {
    const calls: { method: string; url: string; body?: string }[] = [];
    installFetch(async (url, init) => {
      const method = (init?.method ?? 'GET').toUpperCase();
      const body = typeof init?.body === 'string' ? init.body : init?.body?.toString();
      calls.push({ method, url, body });

      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}`)) return fakeResponse(readyCluster());
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}/addons`)) {
        if (method === 'POST') {
          return fakeResponse(
            {
              id: 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa',
              addon: 'ingress-nginx',
              version: '4.10.0',
              helm_release: 'ingress-nginx',
              status: 'installing',
              status_reason: null,
              created_at: '2026-04-30T10:02:00Z',
              updated_at: '2026-04-30T10:02:00Z'
            },
            202
          );
        }
        return fakeResponse([]);
      }
      if (url.endsWith('/v1/catalog/clusters'))
        return fakeResponse({
          regions: ['nbg1'],
          server_types: ['cpx21'],
          addons: ['ingress-nginx']
        });
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(StatusPage);

    await fireEvent.click(await findByTestId('install-ingress-nginx'));
    await fireEvent.click(await findByTestId('install-confirm'));

    await waitFor(() => {
      expect(
        calls.some(
          (c) =>
            c.method === 'POST' &&
            c.url.endsWith(`/v1/clusters/${CLUSTER_ID}/addons`) &&
            c.body?.includes('ingress-nginx') &&
            c.body?.includes('4.10.0')
        )
      ).toBe(true);
    });
  });

  it('renders the sanitised status_reason for a failed addon — never raw helm text', async () => {
    installFetch(async (url) => {
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}`)) return fakeResponse(readyCluster());
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}/addons`))
        return fakeResponse([
          {
            id: 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb',
            addon: 'ingress-nginx',
            version: '4.10.0',
            helm_release: 'ingress-nginx',
            status: 'failed',
            // The Rust runner formats helm errors as `helm: …`, never
            // raw stderr. The UI just renders that string verbatim.
            status_reason: 'helm: NonZeroExit code=1',
            created_at: '2026-04-30T10:02:00Z',
            updated_at: '2026-04-30T10:03:00Z'
          }
        ]);
      if (url.endsWith('/v1/catalog/clusters'))
        return fakeResponse({
          regions: ['nbg1'],
          server_types: ['cpx21'],
          addons: ['ingress-nginx']
        });
      return fakeResponse({}, 404);
    });

    const { findByTestId } = render(StatusPage);

    const row = await findByTestId('addon-row-ingress-nginx');
    expect(row.textContent ?? '').toContain('helm: NonZeroExit code=1');
    // Belt + suspenders: the literal "stderr" word that would show up
    // if we ever leaked raw Helm stderr through must not appear.
    expect(row.textContent ?? '').not.toMatch(/stderr/i);
  });
});

describe('cluster status page — SSE wiring (ticket 10)', () => {
  function provisioningCluster(step: string | null = null): Record<string, unknown> {
    return {
      id: CLUSTER_ID,
      name: 'acme-prod',
      region: 'nbg1',
      server_type: 'cpx21',
      control_plane_count: 1,
      worker_count: 1,
      status: 'provisioning',
      current_step: step,
      started_at: '2026-04-30T10:00:00Z',
      error_category: null,
      terminal: false,
      kubeconfig_available: false,
      created_at: '2026-04-30T10:00:00Z',
      updated_at: '2026-04-30T10:00:00Z'
    };
  }

  it('opens an EventSource and refetches on step events', async () => {
    let currentStep: string | null = null;
    let getCount = 0;
    installFetch(async (url) => {
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}`)) {
        getCount += 1;
        return fakeResponse(provisioningCluster(currentStep));
      }
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}/addons`)) return fakeResponse([]);
      if (url.endsWith('/v1/catalog/clusters'))
        return fakeResponse({
          regions: ['nbg1'],
          server_types: ['cpx21'],
          addons: ['ingress-nginx']
        });
      return fakeResponse({}, 404);
    });

    render(StatusPage);

    // EventSource should be opened on mount, against the correct URL.
    await waitFor(() => {
      expect(FakeEventSource.instances).toHaveLength(1);
    });
    expect(FakeEventSource.instances[0].url).toBe(`/api/v1/clusters/${CLUSTER_ID}/events`);

    // Initial fetch happens too. After a step event, the page should
    // refetch — exposing the new `current_step` to the user.
    await waitFor(() => expect(getCount).toBeGreaterThanOrEqual(1));
    const before = getCount;

    currentStep = 'installing_k3s_server';
    FakeEventSource.instances[0].emit('step');

    await waitFor(() => expect(getCount).toBeGreaterThan(before));
  });

  it('closes the EventSource after a terminal event without polling kicking in', async () => {
    let status = 'provisioning';
    installFetch(async (url) => {
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}`))
        return fakeResponse({
          ...provisioningCluster(),
          status,
          terminal: status !== 'provisioning'
        });
      if (url.endsWith(`/v1/clusters/${CLUSTER_ID}/addons`)) return fakeResponse([]);
      if (url.endsWith('/v1/catalog/clusters'))
        return fakeResponse({
          regions: ['nbg1'],
          server_types: ['cpx21'],
          addons: ['ingress-nginx']
        });
      return fakeResponse({}, 404);
    });

    render(StatusPage);

    await waitFor(() => expect(FakeEventSource.instances).toHaveLength(1));
    const es = FakeEventSource.instances[0];

    status = 'ready';
    es.emit('terminal');

    await waitFor(() => expect(es.closed).toBe(true));
    // No second EventSource should be created — terminal means we're
    // done, not "reconnect".
    expect(FakeEventSource.instances).toHaveLength(1);
  });

  it('falls back to polling after repeated SSE failures', async () => {
    vi.useFakeTimers();
    try {
      const calls: string[] = [];
      installFetch(async (url) => {
        calls.push(url);
        if (url.endsWith(`/v1/clusters/${CLUSTER_ID}`)) return fakeResponse(provisioningCluster());
        if (url.endsWith(`/v1/clusters/${CLUSTER_ID}/addons`)) return fakeResponse([]);
        if (url.endsWith('/v1/catalog/clusters'))
          return fakeResponse({
            regions: ['nbg1'],
            server_types: ['cpx21'],
            addons: ['ingress-nginx']
          });
        return fakeResponse({}, 404);
      });

      render(StatusPage);
      await vi.waitFor(() => expect(FakeEventSource.instances.length).toBe(1));

      // Three consecutive failures before fallback (matches the
      // SSE_FALLBACK_AFTER_RETRIES constant in the page). Each error
      // schedules a backoff timer; advance past it to drive the next
      // attempt synchronously.
      for (let i = 0; i < 4; i += 1) {
        FakeEventSource.instances[FakeEventSource.instances.length - 1].triggerError();
        await vi.advanceTimersByTimeAsync(120_000);
      }

      // After the fourth failure the page is in polling mode. Drive
      // the poll interval and assert another GET fires.
      const beforePollCount = calls.filter((u) => u.endsWith(`/v1/clusters/${CLUSTER_ID}`)).length;
      await vi.advanceTimersByTimeAsync(2_500);
      const afterPollCount = calls.filter((u) => u.endsWith(`/v1/clusters/${CLUSTER_ID}`)).length;
      expect(afterPollCount).toBeGreaterThan(beforePollCount);
    } finally {
      vi.useRealTimers();
    }
  });
});
