/**
 * MFA state banner — covers both the enrol and assert states.
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

  it('shows the assert banner when mfaState is must_assert', () => {
    const { getByTestId } = render(MfaBanner, { props: { mfaState: 'must_assert' } });
    const banner = getByTestId('mfa-assert-banner');
    expect(banner).toBeInTheDocument();
    expect(banner).toHaveAttribute('role', 'alert');
    expect(banner.textContent).toContain('Verify with your passkey');
    const link = banner.querySelector('a');
    expect(link).toHaveAttribute('href', '/app/settings/security?mfa=required');
  });

  it.each<MfaState | null>(['enrolled', 'not_required', null])(
    'renders no banner for mfaState=%s',
    (state) => {
      const { queryByTestId } = render(MfaBanner, { props: { mfaState: state } });
      expect(queryByTestId('mfa-enrol-banner')).toBeNull();
      expect(queryByTestId('mfa-assert-banner')).toBeNull();
    }
  );
});
