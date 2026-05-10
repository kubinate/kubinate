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

export async function listAuditLog(
  organizationId: string,
  fetchImpl: typeof fetch = fetch
): Promise<AuditLogEntry[]> {
  const response = await fetchImpl(`/api/v1/organizations/${organizationId}/audit-log`);
  return parseResponse(response, z.array(auditLogEntrySchema));
}
