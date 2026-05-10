import { meViewSchema, type MeView } from './schemas';

/** `GET /v1/me` — resolves the actor for the cookie session. */
export async function getMe(fetchImpl: typeof fetch = fetch): Promise<MeView> {
  const response = await fetchImpl('/api/v1/me');
  if (!response.ok) {
    throw new Error(`me: ${response.status} ${response.statusText}`);
  }
  return meViewSchema.parse(await response.json());
}

/** `PATCH /v1/me` — update the calling user's display name. */
export async function updateMe(
  displayName: string,
  fetchImpl: typeof fetch = fetch
): Promise<MeView> {
  const res = await fetchImpl('/api/v1/me', {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ display_name: displayName })
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error((body as { detail?: string }).detail ?? `Update failed: ${res.status}`);
  }
  return meViewSchema.parse(await res.json());
}
