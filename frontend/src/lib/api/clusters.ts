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

export async function loadClusterCatalog(
  fetchImpl: typeof fetch = fetch
): Promise<{ regions: string[]; server_types: string[]; addons: string[] }> {
  const response = await fetchImpl('/api/v1/catalog/clusters');
  return parseResponse(response, catalogSchema);
}
