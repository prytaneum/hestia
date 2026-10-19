import { screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { PlayBar } from '@/components/app-shell/play-bar';
import { channels } from '@/mock/channels';
import { m } from '@/paraglide/messages.js';
import { queryClient } from '@/queries';
import { keys } from '@/queries/keys';
import {
  renderWithProviders,
  resetJobs,
  resetQueryCache,
} from '../../../support';

const recent = channels['instance.recent'];

afterEach(async () => {
  channels['instance.recent'] = recent;
  await resetJobs();
  resetQueryCache();
});

const bar = () => renderWithProviders(<PlayBar />, { route: true });

const recentLoaded = () =>
  waitFor(() =>
    expect(queryClient.getQueryState(keys.instances.recent())?.status).toBe(
      'success',
    ),
  );

describe('the play bar', () => {
  it('leads with the most recently played target', async () => {
    bar();
    const plays = recent?.({}) ?? [];

    await waitFor(() => expect(screen.getByText(plays[0].name)).toBeDefined());
    expect(
      screen.getByText(new RegExp(`^${m['app.playbar.kind.world']()} · `)),
    ).toBeDefined();
    expect(screen.getByText(m['app.action.join']())).toBeDefined();
  });

  it('is absent while nothing has been played', async () => {
    channels['instance.recent'] = () => [];
    bar();

    await recentLoaded();
    expect(screen.queryByText(m['app.action.join']())).toBeNull();
  });
});
