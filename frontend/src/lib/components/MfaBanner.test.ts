/**
 * Sprint 5 ticket 07 — MFA state banner.
 *
 * The banner renders only for `must_enrol`; all other states
 * (including null) produce no DOM output.
 */

import { render } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import MfaBanner from './MfaBanner.svelte';
import type { MfaState } from '$lib/api/schemas';

describe('MfaBanner', () => {
  it('shows the enrol banner when mfaState is must_enrol', () => {
    const { getByTestId } = render(MfaBanner, { props: { mfaState: 'must_enrol' } });
    const banner = getByTestId('mfa-enrol-banner');
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveAttribute('role', 'alert');
    expect(banner.textContent).toContain('Set up a passkey');
    const link = banner.querySelector('a');
    expect(link).toHaveAttribute('href', '/app/settings/security');
  });

  it.each<MfaState | null>(['enrolled', 'must_assert', 'not_required', null])(
    'renders nothing for mfaState=%s',
    (state) => {
      const { queryByTestId } = render(MfaBanner, { props: { mfaState: state } });
      expect(queryByTestId('mfa-enrol-banner')).toBeNull();
    }
  );
});
