import { z } from 'zod';
import {
  acceptInviteResponseSchema,
  createInviteResponseSchema,
  inviteViewSchema,
  membershipViewSchema,
  problemDetailsSchema,
  type AcceptInviteResponse,
  type CreateInviteResponse,
  type InviteView,
  type MembershipRole,
  type MembershipView
} from './schemas';

/**
 * Mirrors the `ApiError` from `_problem.ts`. Carried independently
 * here to keep import surfaces narrow per page, but extended with
 * the `code` field (Sprint 4 ticket 05) so callers can branch on
 * `isMfaRequired()` without importing from `_problem` directly.
 */
export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly title: string,
    public readonly detail: string,
    public readonly code?: string
  ) {
    super(`${status} ${title}: ${detail}`);
    this.name = 'ApiError';
  }

  isMfaRequired(): boolean {
    return this.code === 'mfa_required';
  }
}

async function parseResponse<T>(response: Response, schema: z.ZodTypeAny | null): Promise<T> {
  const text = await response.text();
  if (!response.ok) {
    try {
      const problem = problemDetailsSchema.parse(JSON.parse(text));
      throw new ApiError(problem.status, problem.title, problem.detail, problem.code);
    } catch (err) {
      if (err instanceof ApiError) {
        throw err;
      }
      throw new ApiError(response.status, response.statusText, text || 'request failed');
    }
  }
  if (schema === null) {
    return undefined as T;
  }
  return schema.parse(text ? JSON.parse(text) : null) as T;
}

export async function listMembers(
  organizationId: string,
  fetchImpl: typeof fetch = fetch
): Promise<MembershipView[]> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/members`);
  return parseResponse(response, z.array(membershipViewSchema));
}

export async function listInvites(
  organizationId: string,
  fetchImpl: typeof fetch = fetch
): Promise<InviteView[]> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/invites`);
  return parseResponse(response, z.array(inviteViewSchema));
}

export async function createInvite(
  organizationId: string,
  email: string,
  role: MembershipRole,
  fetchImpl: typeof fetch = fetch
): Promise<CreateInviteResponse> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/invites`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email, role })
  });
  return parseResponse(response, createInviteResponseSchema);
}

export async function revokeInvite(
  organizationId: string,
  inviteId: string,
  fetchImpl: typeof fetch = fetch
): Promise<void> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/invites/${inviteId}`, {
    method: 'DELETE'
  });
  return parseResponse(response, null);
}

export async function updateMemberRole(
  organizationId: string,
  userId: string,
  role: MembershipRole,
  fetchImpl: typeof fetch = fetch
): Promise<void> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/members/${userId}`, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ role })
  });
  return parseResponse(response, null);
}

export async function removeMember(
  organizationId: string,
  userId: string,
  fetchImpl: typeof fetch = fetch
): Promise<void> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/members/${userId}`, {
    method: 'DELETE'
  });
  return parseResponse(response, null);
}

export async function acceptInvite(
  token: string,
  fetchImpl: typeof fetch = fetch
): Promise<AcceptInviteResponse> {
  const response = await fetchImpl('/api/v1/invites/accept', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ token })
  });
  return parseResponse(response, acceptInviteResponseSchema);
}
