import { screen, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { StatusBar } from '@/components/app-shell/status-bar';
import { clearNotices } from '@/lib/notifications';
import { notify } from '@/lib/notify';
import { status as daemonStatus } from '@/mock/channels/app';
import { m } from '@/paraglide/messages.js';
import { foregroundJob, type JobMeta, startJob } from '@/queries/jobs';
import {
  pendingJob,
  renderWithProviders,
  resetJobs,
  resetQueryCache,
} from '../../../support';

const meta = (over: Partial<JobMeta> = {}): JobMeta => ({
  channel: 'server.create',
  ...over,
});

const bell = (summary?: string) =>
  screen.queryByRole('button', {
    name: summary
      ? m['app.notifications.summary']({ summary })
      : m['app.notifications.label'](),
  });

afterEach(async () => {
  await resetJobs();
  clearNotices();
  resetQueryCache();
});

describe('the status bar', () => {
  it('reports the daemon it is talking to', async () => {
    renderWithProviders(<StatusBar />);
    expect(screen.getByText(m['app.daemon.connected']())).toBeDefined();
    await waitFor(() =>
      expect(
        screen.getByText(`v${daemonStatus().version}`),
      ).toBeDefined(),
    );
  });

  it('counts the running background jobs on the bell', async () => {
    startJob(meta(), pendingJob);
    startJob(meta(), pendingJob);
    renderWithProviders(<StatusBar />);
    const running = m['app.notifications.running_count']({ count: 2 });
    await waitFor(() => expect(bell(running)).not.toBeNull());
  });

  it('leaves a job a modal has taken into the foreground out of the count', async () => {
    const { id } = startJob(meta(), pendingJob);
    renderWithProviders(<StatusBar />);
    const running = m['app.notifications.running_count']({ count: 1 });
    await waitFor(() => expect(bell(running)).not.toBeNull());

    foregroundJob(id);
    await waitFor(() => expect(bell()).not.toBeNull());
  });

  it('says how many notifications are unread', async () => {
    renderWithProviders(<StatusBar />);
    notify.success('SMP backed up', { quiet: true });
    const unread = m['app.notifications.unread_count']({ count: 1 });
    await waitFor(() => expect(bell(unread)).not.toBeNull());
  });
});
