import { screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { RecentPlaysGroup } from '@/components/app-shell/recent-plays';
import {
  DropdownMenu,
  DropdownMenuContent,
} from '@/components/ui/dropdown-menu';
import { LaunchDialogProvider } from '@/features/instances/dialogs';
import { listInstances } from '@/mock/state/entries';
import { worldsOf } from '@/mock/state/worlds';
import {
  renderWithProviders,
  resetJobs,
  resetQueryCache,
} from '../../../support';

afterEach(async () => {
  await resetJobs();
  resetQueryCache();
});

const menu = () =>
  renderWithProviders(
    <LaunchDialogProvider>
      <DropdownMenu open>
        <DropdownMenuContent>
          <RecentPlaysGroup />
        </DropdownMenuContent>
      </DropdownMenu>
    </LaunchDialogProvider>,
  );

describe('the recently played group of the quick-play menu', () => {
  it('offers at most five targets, each naming its world and its instance', async () => {
    menu();
    const instance = listInstances().find(
      (candidate) => worldsOf(candidate.id).length > 0,
    );
    if (!instance) throw new Error('the mock has an instance with a world');
    const world = worldsOf(instance.id)[0];

    const item = await waitFor(() =>
      screen.getByRole('menuitem', {
        name: new RegExp(`${world.name}.*${instance.name}`),
      }),
    );
    expect(item).toBeDefined();
    expect(screen.getAllByRole('menuitem').length).toBeLessThanOrEqual(5);
  });
});
