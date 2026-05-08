import { meViewSchema, type MeView } from './schemas';

/** `GET /v1/me` — resolves the actor for the cookie session. */
export async function getMe(fetchImpl: typeof fetch = fetch): Promise<MeView> {
  const response = await fetchImpl('/api/v1/me');
  if (!response.ok) {
    throw new Error(`me: ${response.status} ${response.statusText}`);
  }
  return meViewSchema.parse(await response.json());
}
