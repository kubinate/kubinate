import { z } from 'zod';

// Must stay in sync with `kubinate_cluster::service::{ALLOWED_REGIONS,
// ALLOWED_SERVER_TYPES}`. The frontend hydrates these from
// `GET /v1/catalog/clusters` on page load, but having them here as
// fallbacks keeps the form responsive and typed at build time.

export const FALLBACK_REGIONS = ['nbg1', 'fsn1', 'hel1', 'ash', 'hil'] as const;
export const FALLBACK_SERVER_TYPES = [
  'cpx11',
  'cpx21',
  'cpx31',
  'cpx41',
  'ccx13',
  'ccx23'
] as const;

export const createClusterSchema = z.object({
  name: z
    .string()
    .trim()
    .min(1, 'Name is required')
    .max(63, 'Name must be 63 characters or fewer')
    .regex(
      /^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/,
      'Lowercase letters, digits, and dashes only; must start and end alphanumeric'
    ),
  region: z.string().min(1, 'Region is required'),
  server_type: z.string().min(1, 'Server type is required'),
  control_plane_count: z.literal(1, {
    errorMap: () => ({ message: 'Sprint 1 ships single-node control planes only' })
  }),
  worker_count: z
    .number({ invalid_type_error: 'Worker count must be a number' })
    .int('Worker count must be a whole number')
    .min(1, 'At least one worker')
    .max(10, 'At most ten workers in Sprint 1'),
  credential_id: z.string().uuid('Pick a stored Hetzner credential')
});

export type CreateClusterInput = z.infer<typeof createClusterSchema>;

export const errorCategorySchema = z.enum([
  'provider_error',
  'network_error',
  'boot_timeout',
  'installation_error',
  'unknown'
]);
export type ErrorCategory = z.infer<typeof errorCategorySchema>;

export const clusterViewSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  region: z.string(),
  server_type: z.string(),
  control_plane_count: z.number(),
  worker_count: z.number(),
  status: z.enum([
    'pending',
    'provisioning',
    'ready',
    'scaling',
    'failed',
    'destroying',
    'destroyed'
  ]),
  current_step: z.string().nullable().optional(),
  started_at: z.string(),
  error_category: errorCategorySchema.nullable().optional(),
  terminal: z.boolean(),
  kubeconfig_available: z.boolean(),
  created_at: z.string(),
  updated_at: z.string(),
  agent_last_seen_at: z.string().nullable().optional(),
  agent_version: z.string().optional()
});

export type ClusterView = z.infer<typeof clusterViewSchema>;

/** Map an `error_category` onto the user-facing copy the dashboard renders. */
export function errorCategoryMessage(category: ErrorCategory): {
  title: string;
  detail: string;
  retryable: boolean;
} {
  switch (category) {
    case 'provider_error':
      return {
        title: 'Hetzner refused the request',
        detail:
          'This usually means a quota, billing, or token-permission issue. Check your Hetzner Cloud Console, then try again.',
        retryable: true
      };
    case 'network_error':
      return {
        title: 'Transient network issue',
        detail: 'Hetzner returned a temporary failure. Re-running typically succeeds.',
        retryable: true
      };
    case 'boot_timeout':
      return {
        title: 'A node took too long to boot',
        detail:
          'cloud-init did not finish within the deadline. The cluster may have orphan servers — destroy and retry.',
        retryable: true
      };
    case 'installation_error':
      return {
        title: 'k3s installation failed on a node',
        detail:
          'A remote command exited non-zero. Destroy the cluster and retry; if it persists, file a support request.',
        retryable: true
      };
    case 'unknown':
    default:
      return {
        title: 'Provisioning failed',
        detail:
          'We could not bucket this error. Destroy the cluster and retry; if it persists, contact support.',
        retryable: true
      };
  }
}

export const catalogSchema = z.object({
  regions: z.array(z.string()).min(1),
  server_types: z.array(z.string()).min(1),
  // Sprint 3 ticket 09 added cert-manager; this list grows server-side.
  addons: z.array(z.string()).default([])
});

// -----------------------------------------------------------------------------
// Cluster add-ons (Sprint 2 ticket 07 + Sprint 3 ticket 03 UI)
// -----------------------------------------------------------------------------

export const addonStatusSchema = z.enum([
  'pending',
  'installing',
  'ready',
  'failed',
  'uninstalling',
  'uninstalled'
]);
export type AddonStatus = z.infer<typeof addonStatusSchema>;

export const addonViewSchema = z.object({
  id: z.string().uuid(),
  addon: z.string(),
  version: z.string(),
  helm_release: z.string(),
  status: addonStatusSchema,
  status_reason: z.string().nullable().optional(),
  created_at: z.string(),
  updated_at: z.string()
});
export type AddonView = z.infer<typeof addonViewSchema>;
export const addonListSchema = z.array(addonViewSchema);

/** Non-sensitive view of a stored Hetzner credential. */
export const credentialViewSchema = z.object({
  id: z.string().uuid(),
  alias: z.string(),
  created_at: z.string(),
  updated_at: z.string()
});
export type CredentialView = z.infer<typeof credentialViewSchema>;
export const credentialListSchema = z.array(credentialViewSchema);

// -----------------------------------------------------------------------------
// Team / membership views (Sprint 2 ticket 06)
// -----------------------------------------------------------------------------

export const membershipRoleSchema = z.enum(['owner', 'admin', 'developer', 'viewer']);
export type MembershipRole = z.infer<typeof membershipRoleSchema>;

export const acceptInviteResponseSchema = z.object({
  organization_id: z.string().uuid(),
  role: membershipRoleSchema
});
export type AcceptInviteResponse = z.infer<typeof acceptInviteResponseSchema>;

export const membershipViewSchema = z.object({
  id: z.string().uuid(),
  user_id: z.string().uuid(),
  email: z.string(),
  display_name: z.string(),
  role: membershipRoleSchema,
  joined_at: z.string(),
  last_active_at: z.string().nullable().optional()
});
export type MembershipView = z.infer<typeof membershipViewSchema>;

export const inviteViewSchema = z.object({
  id: z.string().uuid(),
  email: z.string(),
  role: membershipRoleSchema,
  token_prefix: z.string(),
  accepted_at: z.string().nullable().optional(),
  revoked_at: z.string().nullable().optional(),
  expires_at: z.string(),
  created_at: z.string()
});
export type InviteView = z.infer<typeof inviteViewSchema>;

export const createInviteResponseSchema = z.object({
  invite: inviteViewSchema,
  /** Returned exactly once by the server. */
  token: z.string()
});
export type CreateInviteResponse = z.infer<typeof createInviteResponseSchema>;

/**
 * Four-state MFA hint surfaced via `/v1/me` so the dashboard can
 * route the user without inferring state from a 401 round-trip.
 *
 * - `not_required` — Member / Developer / Viewer; voluntary MFA only.
 * - `must_enrol` — Owner/Admin with no passkey registered. Banner
 *   prompting `/app/settings/security`.
 * - `must_assert` — Owner/Admin with a passkey but the current
 *   session hasn't satisfied the assertion. Route to the challenge.
 * - `enrolled` — Owner/Admin, passkey registered, session satisfied.
 *   Dashboard runs unrestricted.
 */
export const mfaStateSchema = z.enum(['not_required', 'must_enrol', 'must_assert', 'enrolled']);
export type MfaState = z.infer<typeof mfaStateSchema>;

/** What `GET /v1/me` returns — the resolved actor + MFA state. */
export const meViewSchema = z.object({
  user_id: z.string().uuid(),
  session_id: z.string().uuid(),
  organization_id: z.string().uuid(),
  mfa_state: mfaStateSchema
});
export type MeView = z.infer<typeof meViewSchema>;

// -----------------------------------------------------------------------------
// Billing (Sprint 2 ticket 08 + Sprint 3 ticket 04 UI)
// -----------------------------------------------------------------------------

export const billingPlanSchema = z.enum(['free', 'starter', 'pro']);
export type BillingPlan = z.infer<typeof billingPlanSchema>;

export const billingStateSchema = z.object({
  plan: billingPlanSchema,
  stripe_customer_id: z.string().nullable().optional()
});
export type BillingState = z.infer<typeof billingStateSchema>;

export const checkoutResponseSchema = z.object({
  url: z.string().url()
});
export type CheckoutResponse = z.infer<typeof checkoutResponseSchema>;

/** RFC 7807 shape produced by the Rust API. */
export const problemDetailsSchema = z.object({
  type: z.string(),
  title: z.string(),
  status: z.number(),
  detail: z.string(),
  // Sprint 4 ticket 05 — structured discriminator the SPA branches
  // on. `mfa_required` is the only value the API emits today;
  // future codes (rate-limited, payment-required, ...) land here.
  code: z.string().optional()
});

// -----------------------------------------------------------------------------
// WebAuthn / MFA (Sprint 4 ticket 05)
// -----------------------------------------------------------------------------

export const passkeyViewSchema = z.object({
  id: z.string().uuid(),
  nickname: z.string(),
  registered_at: z.string(),
  last_used_at: z.string().nullable().optional()
});
export type PasskeyView = z.infer<typeof passkeyViewSchema>;

/**
 * The Rust API hands the browser the literal `webauthn-rs`
 * `CreationChallengeResponse` / `RequestChallengeResponse` JSON. We
 * keep them as `unknown`-typed pass-throughs at this layer because
 * the conversion to `PublicKeyCredentialCreationOptions` /
 * `PublicKeyCredentialRequestOptions` (Base64URL → ArrayBuffer)
 * happens at the call site via a small helper rather than via Zod
 * — the spec types have nested ArrayBuffers Zod can't validate.
 */
export const registerStartResponseSchema = z.object({
  ceremony_id: z.string().uuid(),
  challenge: z.unknown()
});
export type RegisterStartResponse = z.infer<typeof registerStartResponseSchema>;

export const assertStartResponseSchema = z.object({
  ceremony_id: z.string().uuid(),
  challenge: z.unknown()
});
export type AssertStartResponse = z.infer<typeof assertStartResponseSchema>;

export const assertFinishResponseSchema = z.object({
  mfa_satisfied: z.boolean()
});
export type AssertFinishResponse = z.infer<typeof assertFinishResponseSchema>;

export const recoveryRegenerateResponseSchema = z.object({
  // Plaintext codes returned exactly once. The page surfaces them
  // with a "save these now" treatment; the server cannot return them
  // again.
  codes: z.array(z.string()).min(1),
  total: z.number()
});
export type RecoveryRegenerateResponse = z.infer<typeof recoveryRegenerateResponseSchema>;

export const recoveryRedeemResponseSchema = z.object({
  mfa_satisfied: z.boolean(),
  remaining: z.number()
});
export type RecoveryRedeemResponse = z.infer<typeof recoveryRedeemResponseSchema>;
