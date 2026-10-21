import { screen, waitFor } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { SignInGate } from '@/components/sign-in-gate';
import { player } from '@/mock/channels/account';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { renderWithProviders, resetQueryCache } from '../../support';

afterEach(resetQueryCache);

describe('SignInGate', () => {
  it('signs the account in when the action is pressed', async () => {
    function Probe() {
      const { active, signedIn } = useAccounts();
      return <span data-testid="who">{signedIn ? active?.name : '-'}</span>;
    }

    renderWithProviders(
      <>
        <SignInGate title="Sign in" hint="Skins need it" />
        <Probe />
      </>,
    );
    await userEvent.click(
      screen.getByRole('button', { name: m['account.sign_in']() }),
    );

    await waitFor(() =>
      expect(screen.getByTestId('who').textContent).toBe(player.name),
    );
  });
});
