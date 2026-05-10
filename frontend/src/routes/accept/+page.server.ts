import { redirect } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ locals, url }) => {
  const token = url.searchParams.get('token') ?? '';
  if (!locals.session) {
    const loginUrl = `/api/v1/auth/github/start?redirect_to=${encodeURIComponent(url.pathname + url.search)}`;
    throw redirect(302, loginUrl);
  }
  return { token };
};
