/**
 * App dashboard page — cluster list with empty and populated states.
 *
 * Three specs:
 *   1. Empty state: renders "No clusters yet" and a "Create cluster" link.
 *   2. Populated: a cluster card shows the name, status badge, and region.
 *   3. Destructive status: a "failed" cluster shows the "failed" badge text.
 *
 * The page receives clusters via the SvelteKit `data` prop (Svelte 5 runes).
 * PageData is a merge of the layout server data (session + mfa_state) and the
 * page load data (clusters). Both must be present for the type to satisfy the
 * generated PageData type.
 *
 * No API calls are made — data is injected directly.
 */

import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import type { ClusterView } from '$lib/api/schemas';
import type { PageData } from './$types';
import AppPage from './+page.svelte';

/** Minimal session stub that satisfies the layout server return shape. */
const stubSession: PageData['session'] = {
  userId: '00000000-0000-0000-0000-000000000001',
  organizationId: '00000000-0000-0000-0000-000000000002'
};

function makeData(clusters: ClusterView[]): PageData {
  return { clusters, session: stubSession, mfa_state: 'not_required' };
}

function makeCluster(overrides: Partial<ClusterView> = {}): ClusterView {
  return {
    id: 'abc-123',
    name: 'prod-eu',
    status: 'ready',
    region: 'nbg1',
    server_type: 'cx21',
    control_plane_count: 1,
    worker_count: 3,
    terminal: false,
    kubeconfig_available: true,
    started_at: '2026-01-01T00:00:00Z',
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides
  };
}

describe('app dashboard page', () => {
  it('shows the empty state when clusters is []', () => {
    const { getByText, getByRole } = render(AppPage, {
      props: { data: makeData([]) }
    });

    expect(getByText('No clusters yet')).toBeInTheDocument();

    const link = getByRole('link', { name: /create cluster/i });
    expect(link).toBeInTheDocument();
    expect(link.getAttribute('href')).toBe('/app/clusters/new');
  });

  it('renders cluster name, status badge, and region for a ready cluster', () => {
    const cluster = makeCluster({
      id: 'abc-123',
      name: 'prod-eu',
      status: 'ready',
      region: 'nbg1'
    });

    const { getByText, getAllByText } = render(AppPage, {
      props: { data: makeData([cluster]) }
    });

    expect(getByText('prod-eu')).toBeInTheDocument();
    // "ready" appears as the badge text; getAllByText handles any duplicate nodes.
    expect(getAllByText(/ready/i).length).toBeGreaterThanOrEqual(1);
    expect(getByText('nbg1')).toBeInTheDocument();
  });

  it('shows the "failed" badge text for a cluster with status "failed"', () => {
    const cluster = makeCluster({ id: 'def-456', name: 'staging-eu', status: 'failed' });

    const { getAllByText } = render(AppPage, {
      props: { data: makeData([cluster]) }
    });

    expect(getAllByText(/failed/i).length).toBeGreaterThanOrEqual(1);
  });
});
