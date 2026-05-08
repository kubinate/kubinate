import { credentialListSchema, type CredentialView } from './schemas';

/**
 * Load every live Hetzner credential the actor's organization has.
 * Used by the cluster-create form to populate the credential picker.
 *
 * Pulls from the existing Sprint 1 ticket-01 endpoint —
 * `GET /v1/integrations/hetzner` returns the non-sensitive shape, no
 * tokens, no `secret_ref`.
 */
export async function listCredentials(fetchImpl: typeof fetch = fetch): Promise<CredentialView[]> {
  const response = await fetchImpl('/api/v1/integrations/hetzner');
  if (!response.ok) {
    throw new Error(`list credentials: ${response.status} ${response.statusText}`);
  }
  return credentialListSchema.parse(await response.json());
}
