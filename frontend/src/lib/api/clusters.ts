import { z } from 'zod';
import {
  catalogSchema,
  clusterViewSchema,
  createClusterSchema,
  type ClusterView,
  type CreateClusterInput
} from './schemas';
import { ApiError, parseProblem } from './_problem';

export { clusterViewSchema };

// Re-export so existing call sites that import from `./clusters`
// keep working unchanged. Sprint 4 ticket 05 moved the canonical
// definition to `_problem.ts` to share `code` parsing across
// modules; `clusters.ts` is still the most-imported source.
export { ApiError };

async function parseResponse<T>(
  response: Response,
  schema: { parse: (u: unknown) => T }
): Promise<T> {
  const text = await response.text();
  if (!response.ok) {
    throw await parseProblem(response, text);
  }
  return schema.parse(text ? JSON.parse(text) : {});
}

/**
 * POST /v1/clusters with an Idempotency-Key so accidental retries
 * (network hiccup, double-click) do not create a second cluster.
 */
export async function createCluster(
  input: CreateClusterInput,
  fetchImpl: typeof fetch = fetch
): Promise<ClusterView> {
  const payload = createClusterSchema.parse(input);
  const response = await fetchImpl('/api/v1/clusters', {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'idempotency-key': crypto.randomUUID()
    },
    body: JSON.stringify(payload)
  });
  return parseResponse(response, clusterViewSchema);
}

export async function getCluster(
  id: string,
  fetchImpl: typeof fetch = fetch
): Promise<ClusterView> {
  const response = await fetchImpl(`/api/v1/clusters/${id}`);
  return parseResponse(response, clusterViewSchema);
}

export async function listClusters(fetchImpl: typeof fetch = fetch): Promise<ClusterView[]> {
  const response = await fetchImpl('/api/v1/clusters');
  return parseResponse(response, z.array(clusterViewSchema));
}

export async function destroyCluster(id: string, fetchImpl: typeof fetch = fetch): Promise<void> {
  const response = await fetchImpl(`/api/v1/clusters/${id}`, { method: 'DELETE' });
  if (!response.ok) {
    const text = await response.text();
    throw await parseProblem(response, text);
  }
}

export async function scaleWorkers(
  id: string,
  delta: number,
  fetchImpl: typeof fetch = fetch
): Promise<void> {
  const response = await fetchImpl(`/api/v1/clusters/${id}/workers`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ delta })
  });
  if (!response.ok) {
    const text = await response.text();
    throw await parseProblem(response, text);
  }
}

export async function loadClusterCatalog(
  fetchImpl: typeof fetch = fetch
): Promise<{ regions: string[]; server_types: string[]; addons: string[] }> {
  const response = await fetchImpl('/api/v1/catalog/clusters');
  return parseResponse(response, catalogSchema);
}

export interface MetricSample {
  labels: { name: string; value: string }[];
  timestamp_ms: number;
  value: number;
}

const metricsResponseSchema = z.object({
  samples: z.array(
    z.object({
      labels: z.array(z.object({ name: z.string(), value: z.string() })),
      timestamp_ms: z.number(),
      value: z.number()
    })
  )
});

export async function getClusterMetrics(
  clusterId: string,
  metric: string,
  startSec: number,
  endSec: number,
  fetchImpl: typeof fetch = fetch
): Promise<MetricSample[]> {
  const params = new URLSearchParams({
    metric,
    start: String(startSec),
    end: String(endSec)
  });
  const response = await fetchImpl(`/api/v1/clusters/${clusterId}/metrics?${params}`);
  const parsed = await parseResponse(response, metricsResponseSchema);
  return parsed.samples;
}
