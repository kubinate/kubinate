import { isRedirect, redirect } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';
import { getMe } from '$lib/api/me';

export const load: LayoutServerLoad = async ({ locals, fetch, url }) => {
  if (!locals.session) {
    throw redirect(302, '/login');
  }

  try {
    const me = await getMe(fetch);

    // Owner/Admin with a passkey but the session hasn't satisfied the
    // assertion yet: gate every /app page except the security page
    // itself (otherwise the user lands in a redirect loop).
    if (me.mfa_state === 'must_assert' && !url.pathname.startsWith('/app/settings/security')) {
      throw redirect(302, '/app/settings/security?mfa=required');
    }

    return {
      session: {
        userId: me.user_id,
        organizationId: me.organization_id,
        displayName: me.display_name,
        email: me.email
      },
      mfa_state: me.mfa_state
    };
  } catch (err) {
    if (isRedirect(err)) throw err;
    // API unreachable in local dev without a running backend:
    // fall through with the stub session so the frontend is still
    // navigable. mfa_state is null, so no banners render.
    return { session: locals.session, mfa_state: null };
  }
};
