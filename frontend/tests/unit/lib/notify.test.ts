import { beforeEach, describe, expect, it, vi } from 'vitest';

const { toast } = vi.hoisted(() => {
  const fn = () => vi.fn();
  return {
    toast: Object.assign(fn(), {
      success: fn(),
      info: fn(),
      warning: fn(),
      error: fn(),
      loading: fn(),
      dismiss: fn(),
    }),
  };
});
vi.mock('sonner', () => ({ toast }));

import { JobCancelled, JobInterrupted } from '@/api';
import { notify } from '@/lib/notify';

const warning = { kind: 'unknown' } as never;

beforeEach(() => vi.clearAllMocks());

describe('notify', () => {
  it('auto-dismisses by severity and keeps errors and actions', () => {
    notify.success('a');
    notify.warning('b');
    notify.error('c');
    notify.info('d', { action: { label: 'Retry', onClick: () => {} } });
    expect(toast.success.mock.calls[0][1].duration).toBe(6_000);
    expect(toast.warning.mock.calls[0][1].duration).toBe(8_000);
    expect(toast.error.mock.calls[0][1].duration).toBe(Infinity);
    expect(toast.info.mock.calls[0][1].duration).toBe(Infinity);
    expect(toast.success.mock.calls[0][1].closeButton).toBe(false);
    expect(toast.error.mock.calls[0][1].closeButton).toBe(true);
    expect(toast.info.mock.calls[0][1].closeButton).toBe(true);
  });

  it('folds warnings and failed items into one toast', () => {
    notify.outcome({
      id: 'job-1',
      title: 'Imported SMP',
      warnings: [warning, warning],
      failures: { failed: 3, total: 12 },
    });
    expect(toast.success).not.toHaveBeenCalled();
    expect(toast.warning).toHaveBeenCalledTimes(1);
    const [title, options] = toast.warning.mock.calls[0];
    expect(title).toBe('Imported SMP');
    expect(options.id).toBe('job-1');
    expect(options.description).toBe('3 of 12 failed · 2 warnings');
    expect(options.action).toBeUndefined();
  });

  it('reports a clean outcome as success', () => {
    notify.outcome({ id: 'job-2', title: 'Exported SMP' });
    expect(toast.success.mock.calls[0][1].description).toBeUndefined();
  });

  it('replaces a repeat under the same id', () => {
    notify.failure(new Error('boom'), { id: 'x' });
    notify.failure(new Error('boom'), { id: 'x' });
    expect(toast.error.mock.calls.map((call) => call[1].id)).toEqual([
      'x',
      'x',
    ]);
  });

  it('shows nothing for a cancelled or interrupted job', () => {
    notify.failure(new JobCancelled('1'), { id: 'x' });
    notify.failure(new JobInterrupted('1'), { id: 'x' });
    expect(toast.error).not.toHaveBeenCalled();
  });

  it('settles a promise under the id it loaded with', async () => {
    await notify.promise(Promise.resolve(1), {
      id: 'p',
      loading: 'Working',
      success: () => 'Done',
    });
    expect(toast.loading.mock.calls[0][1].id).toBe('p');
    expect(toast.success.mock.calls[0][1].id).toBe('p');
  });
});
