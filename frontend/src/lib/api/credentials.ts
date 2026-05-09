import { credentialListSchema, credentialViewSchema, type CredentialView } from './schemas';
import { parseProblem } from './_problem';

export async function listCredentials(fetchImpl: typeof fetch = fetch): Promise<CredentialView[]> {
  const response = await fetchImpl('/api/v1/integrations/hetzner');
  if (!response.ok) throw await parseProblem(response);
  return credentialListSchema.parse(await response.json());
}

export async function createCredential(alias: string, token: string): Promise<CredentialView> {
  const response = await fetch('/api/v1/integrations/hetzner', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ alias, token })
  });
  if (!response.ok) throw await parseProblem(response);
  return credentialViewSchema.parse(await response.json());
}

export async function deleteCredential(id: string): Promise<void> {
  const response = await fetch(`/api/v1/integrations/hetzner/${id}`, { method: 'DELETE' });
  if (!response.ok) throw await parseProblem(response);
}
