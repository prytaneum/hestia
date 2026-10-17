import { afterEach, describe, expect, it, vi } from 'vitest';
import { HestiaError, JobCancelled, jobId, runJob } from '@/api';
import { publish } from '@/mock/bus';
import { installFakeDaemon } from '../../support';

installFakeDaemon();

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

afterEach(() => vi.restoreAllMocks());

const channel = 'server.create';

describe('runJob', () => {
  it('resolves with the done event of its own job', async () => {
    const id = jobId('server.create');
    const outcome = runJob<{ id: string; server: string }>({
      id,
      start: async () => {
        await settle();
        publish('job', { id, channel, state: 'done', server: 'smp' });
      },
    });
    await expect(outcome).resolves.toMatchObject({ id, server: 'smp' });
  });

  it('ignores events belonging to another job', async () => {
    const id = jobId('server.create');
    const outcome = runJob<{ id: string; server: string }>({
      id,
      start: async () => {
        await settle();
        publish('job', {
          id: 'someone-else',
          channel,
          state: 'done',
          server: 'other',
        });
        publish('job', { id, channel, state: 'done', server: 'mine' });
      },
    });
    await expect(outcome).resolves.toMatchObject({ server: 'mine' });
  });

  it('streams progress until it settles', async () => {
    const id = jobId('server.create');
    const seen: number[] = [];
    await runJob<{ id: string }, { percent: number }>({
      id,
      onProgress: (p) => seen.push(p.percent),
      start: async () => {
        await settle();
        publish('job', { id, channel, state: 'progress', percent: 10 });
        publish('job', { id, channel, state: 'progress', percent: 90 });
        publish('job', { id, channel, state: 'done' });
      },
    });
    expect(seen).toEqual([10, 90]);
  });

  it('stops reporting progress once the job is done', async () => {
    const id = jobId('server.create');
    const seen: number[] = [];
    await runJob<{ id: string }, { percent: number }>({
      id,
      onProgress: (p) => seen.push(p.percent),
      start: async () => {
        await settle();
        publish('job', { id, channel, state: 'done' });
        publish('job', { id, channel, state: 'progress', percent: 100 });
      },
    });
    await settle();
    expect(seen).toEqual([]);
  });

  it('rejects with the structured error the daemon reported', async () => {
    const id = jobId('server.create');
    const outcome = runJob<{ id: string }>({
      id,
      start: async () => {
        await settle();
        publish('job', {
          id,
          channel,
          state: 'failed',
          error: { kind: 'already_exists', entry: 'server', name: 'smp' },
        });
      },
    });
    await expect(outcome).rejects.toBeInstanceOf(HestiaError);
    await outcome.catch((error: HestiaError) => {
      expect(error.info?.kind).toBe('already_exists');
    });
  });

  it('distinguishes a cancellation from a failure', async () => {
    const id = jobId('server.create');
    const outcome = runJob<{ id: string }>({
      id,
      start: async () => {
        await settle();
        publish('job', { id, channel, state: 'cancelled' });
      },
    });
    await expect(outcome).rejects.toBeInstanceOf(JobCancelled);
  });

  it('propagates a rejection from the start call itself', async () => {
    const id = jobId('server.create');
    await expect(
      runJob<{ id: string }>({
        id,
        start: () => Promise.reject(new HestiaError('bad_request', 'nope')),
      }),
    ).rejects.toThrow('nope');
  });

  it('hears a terminal event that arrives before start resolves', async () => {
    const id = jobId('server.create');
    const outcome = runJob<{ id: string; server: string }>({
      id,
      start: async () => {
        publish('job', { id, channel, state: 'done', server: 'instant' });
        await settle();
      },
    });
    await expect(outcome).resolves.toMatchObject({ server: 'instant' });
  });
});
