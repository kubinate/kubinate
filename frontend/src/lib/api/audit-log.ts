import { z } from 'zod';
import { auditLogEntrySchema, type AuditLogEntry } from './schemas';
import { parseProblem } from './_problem';

export type { AuditLogEntry };

async function parseResponse<T>(
  response: Response,
  schema: { parse: (u: unknown) => T }
): Promise<T> {
  const text = await response.text();
  if (!response.ok) {
    throw await parseProblem(response, text);
  }
  return schema.parse(text ? JSON.parse(text) : []);
}

export const PAGE_SIZE = 50;

export async function listAuditLog(
  organizationId: string,
  opts: { before?: string; limit?: number } = {},
  fetchImpl: typeof fetch = fetch
): Promise<AuditLogEntry[]> {
  const params = new URLSearchParams();
  if (opts.before) params.set('before', opts.before);
  if (opts.limit) params.set('limit', String(opts.limit));
  const qs = params.size > 0 ? `?${params}` : '';
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/audit-log${qs}`);
  return parseResponse(response, z.array(auditLogEntrySchema));
}
