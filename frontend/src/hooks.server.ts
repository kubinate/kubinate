import type { Handle } from '@sveltejs/kit';

/**
 * Global server-side hook.
 *
 * Responsibilities in Phase 0:
 *  - Generate a request ID and forward it to the backend via X-Request-Id.
 *  - Read the session cookie (if present) and stub `event.locals.session`.
 *
 * Auth handshake with the Rust API lands in Phase 1.
 */
export const handle: Handle = async ({ event, resolve }) => {
  const requestId = event.request.headers.get('x-request-id') ?? crypto.randomUUID();

  // Placeholder: resolve session from cookie in Phase 1.
  const sessionCookie = event.cookies.get('kubinate_session');
  if (sessionCookie) {
    event.locals.session = { userId: 'unknown' };
  }

  const response = await resolve(event);
  response.headers.set('x-request-id', requestId);
  return response;
};
