import { orgViewSchema, type OrgView } from './schemas';

export async function getOrganization(
  orgId: string,
  fetchImpl: typeof fetch = fetch
): Promise<OrgView> {
  const res = await fetchImpl(`/api/v1/organizations/${orgId}`);
  if (!res.ok) throw new Error(`Failed to load organization: ${res.status}`);
  return orgViewSchema.parse(await res.json());
}

export async function updateOrganization(
  orgId: string,
  displayName: string,
  fetchImpl: typeof fetch = fetch
): Promise<OrgView> {
  const res = await fetchImpl(`/api/v1/organizations/${orgId}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ display_name: displayName })
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({}));
    throw new Error((body as { detail?: string }).detail ?? `Update failed: ${res.status}`);
  }
  return orgViewSchema.parse(await res.json());
}
