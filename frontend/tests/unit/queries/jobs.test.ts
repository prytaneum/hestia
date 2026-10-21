import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

vi.mock('@/lib/notify', () => ({
  notify: { failure: vi.fn(), outcome: vi.fn(), warning: vi.fn() },
}));

import {
  HestiaError,
  type JobInfo,
  JobInterrupted,
  type ProvisionProgress,
} from '@/api';
import { notify } from '@/lib/notify';
import { connection, publish, publishJob } from '@/mock/bus';
import { channels } from '@/mock/channels';
import {
  clearSettledJobs,
  foregroundJob,
  getJobs,
  type JobMeta,
  type JobOutcome,
  startJob,
  startJobs,
} from '@/queries/jobs';
import { installFakeDaemon } from '../../support';

const meta = (over: Partial<JobMeta> = {}): JobMeta => ({
  channel: 'server.create',
  ...over,
});

const step = (current: number): ProvisionProgress => ({
  phase: 'resolving',
  current,
  total: 3,
  detail: '',
  item: 0,
  items: 0,
  overallCurrent: 0,
  overallTotal: 0,
});

const nextFrame = () =>
  new Promise((resolve) => requestAnimationFrame(resolve));

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

const find = (id: string) => getJobs().find((job) => job.id === id);

let listed: JobInfo[] = [];

beforeAll(async () => {
  installFakeDaemon();
  channels['job.list'] = () => listed;
  await startJobs();
});

afterEach(() => {
  vi.clearAllMocks();
  listed = [];
  clearSettledJobs();
});

describe('the job store', () => {
  it('registers a started job as running and own', () => {
    const { id } = startJob(meta(), () => new Promise(() => {}));
    expect(find(id)).toMatchObject({
      status: 'running',
      channel: 'server.create',
      origin: 'own',
    });
  });

  it('mints the id and hands it to the run, surviving an await', async () => {
    let seen = '';
    const handle = startJob(meta(), async (job) => {
      await Promise.resolve();
      seen = job.id;
      return 'ok';
    });
    await handle.result;
    expect(seen).toBe(handle.id);
    expect(find(handle.id)).toBeDefined();
  });

  it('settles to done with the run’s value', async () => {
    const handle = startJob(meta(), () => Promise.resolve('landed'));
    await expect(handle.result).resolves.toBe('landed');
    expect(find(handle.id)?.status).toBe('done');
    expect(find(handle.id)?.settledAt).toBeTypeOf('number');
  });

  it('settles to failed when the start call fails, keeping the error', async () => {
    const failure = new HestiaError('handler_error', 'nope');
    const handle = startJob(meta(), () => Promise.reject(failure));
    await expect(handle.result).rejects.toBe(failure);
    expect(find(handle.id)?.status).toBe('failed');
    expect(find(handle.id)?.error).toBe(failure);
  });

  it('wraps a non-daemon rejection so the store always has a code', async () => {
    const handle = startJob(meta(), () => Promise.reject(new Error('boom')));
    await expect(handle.result).rejects.toThrow('boom');
    expect(find(handle.id)?.error).toBeInstanceOf(HestiaError);
  });

  it('coalesces event progress to one frame', async () => {
    const { id } = startJob(meta(), () => new Promise(() => {}));
    publishJob(id, 'server.create', { state: 'progress', ...step(1) });
    publishJob(id, 'server.create', { state: 'progress', ...step(2) });
    publishJob(id, 'server.create', { state: 'progress', ...step(3) });
    await flush();
    expect(find(id)?.progress).toBeNull();

    await nextFrame();
    expect(find(id)?.progress).toMatchObject({ current: 3 });
  });

  it('ignores progress that arrives after the job settled', async () => {
    const { id } = startJob(meta(), () => new Promise(() => {}));
    publishJob(id, 'server.create', { state: 'cancelled' });
    publishJob(id, 'server.create', { state: 'progress', ...step(1) });
    await flush();
    await nextFrame();
    expect(find(id)?.status).toBe('cancelled');
    expect(find(id)?.progress).toBeNull();
  });

  it('adopts a job another client started, with its subject', async () => {
    publish('job', {
      id: 'cli-1',
      channel: 'server.backup.create',
      subject: { kind: 'server', id: 'smp' },
      state: 'progress',
      ...step(1),
    });
    await flush();
    expect(find('cli-1')).toMatchObject({
      origin: 'foreign',
      status: 'running',
      channel: 'server.backup.create',
      subject: { kind: 'server', id: 'smp' },
    });

    publishJob('cli-1', 'server.backup.create', { state: 'cancelled' });
    await flush();
    expect(find('cli-1')?.status).toBe('cancelled');
  });

  it('settles once when the event and the rejection land together', async () => {
    const failure = new HestiaError('transport', 'late');
    let reject!: (error: Error) => void;
    const handle = startJob(
      meta(),
      () =>
        new Promise((_, fail) => {
          reject = fail;
        }),
    );
    publishJob(handle.id, 'server.create', {
      state: 'failed',
      error: { kind: 'already_exists', entry: 'server', name: 'smp' },
    });
    await flush();
    const settledAt = find(handle.id)?.settledAt;
    expect(find(handle.id)?.error?.info).toMatchObject({
      kind: 'already_exists',
    });

    reject(failure);
    await expect(handle.result).rejects.toBe(failure);
    expect(find(handle.id)?.settledAt).toBe(settledAt);
    expect(find(handle.id)?.error).not.toBe(failure);
  });

  it('hydrates: adopts a running job and interrupts a vanished one', async () => {
    const vanished = startJob(
      meta(),
      (job) =>
        new Promise((_, reject) =>
          job.signal?.addEventListener('abort', () =>
            reject(job.signal?.reason),
          ),
        ),
    );
    const interrupted = expect(vanished.result).rejects.toBeInstanceOf(
      JobInterrupted,
    );
    listed = [
      {
        id: 'bg-1',
        channel: 'server.backup.create',
        subject: { kind: 'server', id: 'smp' },
        startedUnix: 1,
        progress: step(2),
      },
    ];

    connection('connected');
    await flush();
    await flush();

    expect(find('bg-1')).toMatchObject({
      origin: 'foreign',
      status: 'running',
      progress: { current: 2 },
    });
    expect(find(vanished.id)?.status).toBe('interrupted');
    await interrupted;
  });

  it('clears settled jobs and leaves running ones', async () => {
    const running = startJob(meta(), () => new Promise(() => {}));
    const settled = startJob(meta(), () => Promise.resolve('ok'));
    await settled.result;

    clearSettledJobs();
    expect(find(settled.id)).toBeUndefined();
    expect(find(running.id)?.status).toBe('running');
  });
});

describe('job outcome notifications', () => {
  const failure = new HestiaError('handler_error', 'nope');
  const done = (over: Partial<JobOutcome> = {}) => ({
    notify: () => ({ title: 'Created SMP', ...over }),
  });

  it('reports a failure once, though the event and the rejection both land', async () => {
    const retry = vi.fn();
    const handle = startJob(meta(), () => Promise.reject(failure), { retry });
    publishJob(handle.id, 'server.create', {
      state: 'failed',
      error: { kind: 'already_exists', entry: 'server', name: 'smp' },
    });
    await expect(handle.result).rejects.toBe(failure);
    expect(notify.failure).toHaveBeenCalledTimes(1);
    const options = vi.mocked(notify.failure).mock.calls[0][1];
    expect(options).toMatchObject({ id: `job:${handle.id}`, quiet: false });
    options.action?.onClick();
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it('says nothing about a cancel', async () => {
    const handle = startJob(meta(), () => new Promise(() => {}), done());
    publishJob(handle.id, 'server.create', { state: 'cancelled' });
    await flush();
    expect(find(handle.id)?.status).toBe('cancelled');
    expect(notify.failure).not.toHaveBeenCalled();
    expect(notify.outcome).not.toHaveBeenCalled();
  });

  it('keeps a foreign failure in the list without a toast or a retry', async () => {
    publishJob('cli-2', 'server.backup.create', {
      state: 'failed',
      error: { kind: 'already_exists', entry: 'server', name: 'smp' },
    });
    await flush();
    expect(find('cli-2')?.status).toBe('failed');
    expect(notify.failure).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ id: 'job:cli-2', quiet: true, action: undefined }),
    );
  });

  it('keeps an interrupted job in the list without a toast', async () => {
    const vanished = startJob(meta(), () => new Promise(() => {}));
    connection('connected');
    await flush();
    await flush();
    expect(find(vanished.id)?.status).toBe('interrupted');
    expect(notify.warning).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({ id: `job:${vanished.id}`, quiet: true }),
    );
  });

  it('toasts the declared outcome when nobody watched', async () => {
    const handle = startJob(meta(), () => Promise.resolve('ok'), done());
    await handle.result;
    expect(notify.outcome).toHaveBeenCalledWith(
      { title: 'Created SMP', id: `job:${handle.id}` },
      { quiet: false },
    );
  });

  it('only records the success of a job a surface displayed, but toasts its warnings', async () => {
    let finish!: (value: string) => void;
    const watched = startJob(
      meta(),
      () => new Promise<string>((resolve) => (finish = resolve)),
      done(),
    );
    foregroundJob(watched.id);
    finish('ok');
    await watched.result;
    expect(vi.mocked(notify.outcome).mock.calls[0][1]).toEqual({ quiet: true });

    const degraded = startJob(
      meta(),
      () =>
        new Promise<string>((resolve) => {
          finish = resolve;
        }),
      done({ warnings: [{ kind: 'unknown' } as never] }),
    );
    foregroundJob(degraded.id);
    finish('ok');
    await degraded.result;
    expect(vi.mocked(notify.outcome).mock.calls[1][1]).toEqual({ quiet: false });
  });

  it('only records a failure a surface renders inline', async () => {
    const handle = startJob(meta(), () => new Promise(() => {}));
    foregroundJob(handle.id, true);
    publishJob(handle.id, 'server.create', {
      state: 'failed',
      error: { kind: 'already_exists', entry: 'server', name: 'smp' },
    });
    await flush();
    expect(vi.mocked(notify.failure).mock.calls[0][1].quiet).toBe(true);
  });
});
