import { addonListSchema, addonViewSchema, problemDetailsSchema, type AddonView } from './schemas';

/** Mirrors the `ApiError` from the other API helpers — kept local so
 *  per-page imports stay narrow. */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly title: string,
    public readonly detail: string
  ) {
    super(`${status} ${title}: ${detail}`);
    this.name = 'ApiError';
  }
}

async function parseResponse<T>(
  response: Response,
  schema: { parse: (u: unknown) => T } | null
): Promise<T> {
  const text = await response.text();
  if (!response.ok) {
    try {
      const problem = problemDetailsSchema.parse(JSON.parse(text));
      throw new ApiError(problem.status, problem.title, problem.detail);
    } catch (err) {
      if (err instanceof ApiError) throw err;
      throw new ApiError(response.status, response.statusText, text || 'request failed');
    }
  }
  if (schema === null) {
    return undefined as T;
  }
  return schema.parse(text ? JSON.parse(text) : null) as T;
}

/** `GET /v1/clusters/:id/addons` */
export async function listAddons(
  clusterId: string,
  fetchImpl: typeof fetch = fetch
): Promise<AddonView[]> {
  const response = await fetchImpl(`/api/v1/clusters/${clusterId}/addons`);
  return parseResponse(response, addonListSchema);
}

/** `POST /v1/clusters/:id/addons` */
export async function installAddon(
  clusterId: string,
  addon: string,
  version: string,
  fetchImpl: typeof fetch = fetch
): Promise<AddonView> {
  const response = await fetchImpl(`/api/v1/clusters/${clusterId}/addons`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ addon, version })
  });
  return parseResponse(response, addonViewSchema);
}

/** `DELETE /v1/clusters/:id/addons/:addonId` */
export async function uninstallAddon(
  clusterId: string,
  addonId: string,
  fetchImpl: typeof fetch = fetch
): Promise<AddonView> {
  const response = await fetchImpl(`/api/v1/clusters/${clusterId}/addons/${addonId}`, {
    method: 'DELETE'
  });
  return parseResponse(response, addonViewSchema);
}
