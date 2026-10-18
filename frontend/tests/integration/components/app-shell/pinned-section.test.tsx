import { screen, waitFor } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { PinnedSection } from '@/components/app-shell/pinned-section';
import { LaunchDialogProvider } from '@/features/instances/dialogs';
import { findServer } from '@/mock/state/entries';
import { serverProcessId } from '@/lib/process-id';
import { isRunning } from '@/mock/state/processes';
import { m } from '@/paraglide/messages.js';
import {
  renderWithProviders,
  resetJobs,
  resetQueryCache,
} from '../../../support';

afterEach(async () => {
  await resetJobs();
  resetQueryCache();
});

async function pinServer(id: string) {
  renderWithProviders(
    <LaunchDialogProvider>
      <PinnedSection pathname="/" />
    </LaunchDialogProvider>,
    { route: true },
  );
  const add = await screen.findByRole('button', {
    name: m['app.label.pin_entries'](),
  });
  await waitFor(() => expect(add.hasAttribute('disabled')).toBe(false));
  await userEvent.click(add);
  await userEvent.click(
    await screen.findByRole('menuitemcheckbox', { name: findServer(id).name }),
  );
  await userEvent.keyboard('{Escape}');
}

describe('a pinned row', () => {
  it('starts a stopped server from its quick-play button, then drops the button', async () => {
    await pinServer('smp');

    await userEvent.click(
      await screen.findByRole('button', { name: m['app.action.start']() }),
    );

    await waitFor(() => expect(isRunning(serverProcessId('smp'))).toBe(true));
    await waitFor(() =>
      expect(
        screen.queryByRole('button', { name: m['app.action.start']() }),
      ).toBeNull(),
    );
  });
});
