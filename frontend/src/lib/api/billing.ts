import {
  billingStateSchema,
  checkoutResponseSchema,
  problemDetailsSchema,
  type BillingPlan,
  type BillingState,
  type CheckoutResponse
} from './schemas';

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly title: string,
    public readonly detail: string
  ) {
    super(`${status} ${title}: ${detail}`);
    this.name = 'ApiError';
  }
}

async function parseResponse<T>(
  response: Response,
  schema: { parse: (u: unknown) => T }
): Promise<T> {
  const text = await response.text();
  if (!response.ok) {
    try {
      const problem = problemDetailsSchema.parse(JSON.parse(text));
      throw new ApiError(problem.status, problem.title, problem.detail);
    } catch (err) {
      if (err instanceof ApiError) throw err;
      throw new ApiError(response.status, response.statusText, text || 'request failed');
    }
  }
  return schema.parse(text ? JSON.parse(text) : null);
}

/** `GET /v1/organizations/:id/billing` */
export async function getBillingState(
  organizationId: string,
  fetchImpl: typeof fetch = fetch
): Promise<BillingState> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/billing`);
  return parseResponse(response, billingStateSchema);
}

/** `POST /v1/billing/checkout` */
export async function startCheckout(
  plan: BillingPlan,
  fetchImpl: typeof fetch = fetch
): Promise<CheckoutResponse> {
  const response = await fetchImpl('/api/v1/billing/checkout', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ plan })
  });
  return parseResponse(response, checkoutResponseSchema);
}
