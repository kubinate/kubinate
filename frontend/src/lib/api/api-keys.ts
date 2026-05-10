import { z } from 'zod';
import {
  apiKeyViewSchema,
  createApiKeyResponseSchema,
  type ApiKeyView,
  type CreateApiKeyResponse
} from './schemas';
import { parseProblem } from './_problem';

export type { ApiKeyView, CreateApiKeyResponse };

async function parseResponse<T>(
  response: Response,
  schema: { parse: (u: unknown) => T } | null
): Promise<T> {
  const text = await response.text();
  if (!response.ok) {
    throw await parseProblem(response, text);
  }
  if (schema === null) {
    return undefined as T;
  }
  return schema.parse(text ? JSON.parse(text) : null) as T;
}

/** `GET /v1/api-keys` */
export async function listApiKeys(fetchImpl: typeof fetch = fetch): Promise<ApiKeyView[]> {
  const response = await fetchImpl('/api/v1/api-keys');
  return parseResponse(response, z.array(apiKeyViewSchema));
}

/** `POST /v1/api-keys` — returns view + plaintext token (shown once). */
export async function createApiKey(
  name: string,
  expiresInDays?: number,
  fetchImpl: typeof fetch = fetch
): Promise<CreateApiKeyResponse> {
  const response = await fetchImpl('/api/v1/api-keys', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name, expires_in_days: expiresInDays ?? null })
  });
  return parseResponse(response, createApiKeyResponseSchema);
}

/** `DELETE /v1/api-keys/:id` */
export async function revokeApiKey(id: string, fetchImpl: typeof fetch = fetch): Promise<void> {
  const response = await fetchImpl(`/api/v1/api-keys/${id}`, { method: 'DELETE' });
  return parseResponse(response, null);
}
