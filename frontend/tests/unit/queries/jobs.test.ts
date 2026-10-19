import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import {
  HestiaError,
  type JobInfo,
  JobInterrupted,
  type ProvisionProgress,
} from '@/api';
import { connection, publish, publishJob } from '@/mock/bus';
import { channels } from '@/mock/channels';
import {
  backgroundJob,
  clearSettledJobs,
  dismissJob,
  foregroundJob,
  getJobs,
  type JobMeta,
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

  it('tags the entry a job acts on', () => {
    const { id } = startJob(
      meta({ subject: { kind: 'instance', id: 'inst-1' } }),
      () => new Promise(() => {}),
    );
    expect(find(id)?.subject).toEqual({ kind: 'instance', id: 'inst-1' });
  });

  it('moves a job in and out of the status bar', () => {
    const { id } = startJob(meta(), () => new Promise(() => {}));
    expect(find(id)?.background).toBe(true);
    foregroundJob(id);
    expect(find(id)?.background).toBe(false);
    backgroundJob(id);
    expect(find(id)?.background).toBe(true);
  });

  it('dismisses a settled job but never a running one', async () => {
    const running = startJob(meta(), () => new Promise(() => {}));
    dismissJob(running.id);
    expect(find(running.id)).toBeDefined();

    const settled = startJob(meta(), () => Promise.resolve('ok'));
    await settled.result;
    dismissJob(settled.id);
    expect(find(settled.id)).toBeUndefined();
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
