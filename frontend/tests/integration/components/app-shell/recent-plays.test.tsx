import { screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { RecentPlays } from '@/components/app-shell/recent-plays';
import { LaunchDialogProvider } from '@/features/instances/dialogs';
import { listInstances } from '@/mock/state/entries';
import { worldsOf } from '@/mock/state/worlds';
import { renderWithProviders, resetJobs, resetQueryCache } from '../../../support';

afterEach(async () => {
  await resetJobs();
  resetQueryCache();
});

const strip = () =>
  renderWithProviders(
    <LaunchDialogProvider>
      <RecentPlays />
    </LaunchDialogProvider>,
  );

describe('the recently played strip', () => {
  it('offers at most five targets, each naming its world and its instance', async () => {
    strip();
    const instance = listInstances().find(
      (candidate) => worldsOf(candidate.id).length > 0,
    );
    if (!instance) throw new Error('the mock has an instance with a world');
    const world = worldsOf(instance.id)[0];

    const button = await waitFor(() =>
      screen.getByRole('button', {
        name: new RegExp(`${world.name} · ${instance.name}`),
      }),
    );
    expect(button).toBeDefined();
    expect(screen.getAllByRole('button').length).toBeLessThanOrEqual(5);
  });
});
