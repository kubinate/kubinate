import type { PageLoad } from './$types';
import { listClusters } from '$lib/api/clusters';

export const load: PageLoad = async ({ fetch }) => {
  const clusters = await listClusters(fetch).catch(() => []);
  return { clusters };
};
