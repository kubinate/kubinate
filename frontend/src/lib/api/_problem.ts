/**
 * Shared RFC 7807 Problem Details handling.
 *
 * Sprint 4 ticket 05 added a `code` field — the SPA branches on
 * `code === 'mfa_required'` to redirect to the WebAuthn assertion
 * flow rather than just rendering the error.
 */

import { problemDetailsSchema } from './schemas';

/**
 * Narrow error surface every API client throws on non-2xx
 * responses. Carries the parsed Problem Details fields so
 * callers can branch on `code` without re-parsing.
 */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly title: string,
    public readonly detail: string,
    /**
     * Sprint 4 ticket 05 — structured discriminator. `mfa_required`
     * is the only value the API emits today; future codes
     * (rate-limited, payment-required, ...) extend the union.
     * `undefined` for the pre-Sprint-4 generic Problem Details
     * (`type: "about:blank"`).
     */
    public readonly code?: string
  ) {
    super(`${status} ${title}: ${detail}`);
    this.name = 'ApiError';
  }

  /**
   * Convenience predicate for the partial-MFA-session redirect
   * branch. Equivalent to `code === 'mfa_required'` but reads
   * cleaner at call sites.
   */
  isMfaRequired(): boolean {
    return this.code === 'mfa_required';
  }
}

/**
 * Parse a non-OK Response into an [`ApiError`]. Falls back to
 * plain `statusText` when the body isn't a Problem Details
 * payload (e.g. an upstream proxy returned its own HTML 502).
 *
 * Caller already has the response body? Pass it in via `text` to
 * avoid double-consuming the stream.
 */
export async function parseProblem(
  response: Response,
  text?: string
): Promise<ApiError> {
  const body = text ?? (await response.text());
  if (body) {
    try {
      const problem = problemDetailsSchema.parse(JSON.parse(body));
      return new ApiError(problem.status, problem.title, problem.detail, problem.code);
    } catch {
      // Fall through to the plain-text path.
    }
  }
  return new ApiError(response.status, response.statusText, body || 'request failed');
}
