import type { PageLoad } from './$types';
import { listCredentials } from '$lib/api/credentials';

export const load: PageLoad = async ({ fetch }) => {
  const credentials = await listCredentials(fetch).catch(() => []);
  return { credentials };
};
