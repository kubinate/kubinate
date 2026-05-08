/**
 * Sprint 5 ticket 07 — `mfa_state` four-state matrix.
 *
 * The backend's `/v1/me` returns one of four `mfa_state` values
 * (`not_required` / `must_enrol` / `must_assert` / `enrolled`).
 * This spec pins the schema's accept/reject behaviour so a future
 * API drift (typo, dropped variant) fails loudly at parse time
 * rather than silently routing the user to the wrong UI.
 */

import { describe, expect, it } from 'vitest';
import { meViewSchema, mfaStateSchema, type MfaState } from './schemas';

const VALID_STATES: MfaState[] = ['not_required', 'must_enrol', 'must_assert', 'enrolled'];

const baseMeView = {
  user_id: '0190a000-0000-7000-8000-000000000001',
  session_id: '0190a000-0000-7000-8000-000000000002',
  organization_id: '0190a000-0000-7000-8000-000000000003'
};

describe('mfaStateSchema', () => {
  it.each(VALID_STATES)('accepts the valid value %s', (state) => {
    expect(mfaStateSchema.parse(state)).toBe(state);
  });

  it.each([
    ['empty string', ''],
    ['camelCase variant', 'mustEnrol'],
    ['typo', 'must_enroll'],
    ['unrelated string', 'maybe_required'],
    ['number', 1],
    ['null', null],
    ['undefined', undefined]
  ])('rejects %s', (_label, value) => {
    expect(() => mfaStateSchema.parse(value)).toThrow();
  });
});

describe('meViewSchema with mfa_state', () => {
  it.each(VALID_STATES)('parses a complete /v1/me response with mfa_state=%s', (state) => {
    const parsed = meViewSchema.parse({ ...baseMeView, mfa_state: state });
    expect(parsed.mfa_state).toBe(state);
  });

  it('rejects a /v1/me response that omits mfa_state', () => {
    expect(() => meViewSchema.parse(baseMeView)).toThrow();
  });

  it('rejects a /v1/me response with an unknown mfa_state', () => {
    expect(() => meViewSchema.parse({ ...baseMeView, mfa_state: 'idk' })).toThrow();
  });
});
