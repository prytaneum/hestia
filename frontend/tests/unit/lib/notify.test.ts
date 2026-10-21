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

  it('shows nothing for a cancelled or interrupted job', () => {
    notify.failure(new JobCancelled('1'), { id: 'x' });
    notify.failure(new JobInterrupted('1'), { id: 'x' });
    expect(toast.error).not.toHaveBeenCalled();
  });
});
