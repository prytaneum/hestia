import { afterEach, describe, expect, it } from 'vitest';
import {
  HestiaError,
  JobCancelled,
  jobId,
  type ProvisionProgress,
  runJob,
} from '@/api';
import { publishJob } from '@/mock/bus';
import { channels } from '@/mock/channels';
import { listServers } from '@/mock/state/entries';
import { installFakeDaemon } from '../../support';

installFakeDaemon();

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const server = listServers()[0];
const channel = 'server.create';
const fixture = channels[channel];

const step = (current: number): ProvisionProgress => ({
  phase: 'resolving',
  current,
  total: 2,
  detail: '',
  item: 0,
  items: 0,
});

function onStart(start: () => void) {
  channels[channel] = (payload) => {
    start();
    return { id: String(payload.id) };
  };
}

function later(events: () => void) {
  setTimeout(events, 0);
}

afterEach(() => {
  channels[channel] = fixture;
});

describe('runJob', () => {
  it('starts the channel under its own job id and resolves with the done event', async () => {
    const id = jobId(channel);
    let started = '';
    channels[channel] = (payload) => {
      started = String(payload.id);
      later(() =>
        publishJob(id, channel, {
          state: 'done',
          server,
          warnings: [],
          failures: [],
        }),
      );
      return { id: started };
    };
    await expect(runJob(channel, {}, { id })).resolves.toMatchObject({
      server,
    });
    expect(started).toBe(id);
  });

  it('ignores events belonging to another job', async () => {
    const id = jobId(channel);
    onStart(() =>
      later(() => {
        publishJob('someone-else', channel, {
          state: 'done',
          server: { ...server, name: 'other' },
          warnings: [],
          failures: [],
        });
        publishJob(id, channel, {
          state: 'done',
          server: { ...server, name: 'mine' },
          warnings: [],
          failures: [],
        });
      }),
    );
    const done = await runJob(channel, {}, { id });
    expect(done.server.name).toBe('mine');
  });

  it('streams progress until it settles', async () => {
    const id = jobId(channel);
    const seen: number[] = [];
    onStart(() =>
      later(() => {
        publishJob(id, channel, { state: 'progress', ...step(1) });
        publishJob(id, channel, { state: 'progress', ...step(2) });
        publishJob(id, channel, { state: 'done', server, warnings: [], failures: [] });
      }),
    );
    await runJob(
      channel,
      {},
      { id, onProgress: (progress) => seen.push(progress.current) },
    );
    expect(seen).toEqual([1, 2]);
  });

  it('stops reporting progress once the job is done', async () => {
    const id = jobId(channel);
    const seen: number[] = [];
    onStart(() =>
      later(() => {
        publishJob(id, channel, { state: 'done', server, warnings: [], failures: [] });
        publishJob(id, channel, { state: 'progress', ...step(2) });
      }),
    );
    await runJob(
      channel,
      {},
      { id, onProgress: (progress) => seen.push(progress.current) },
    );
    await settle();
    expect(seen).toEqual([]);
  });

  it('rejects with the structured error the daemon reported', async () => {
    const id = jobId(channel);
    onStart(() =>
      later(() =>
        publishJob(id, channel, {
          state: 'failed',
          error: { kind: 'already_exists', entry: 'server', name: 'smp' },
        }),
      ),
    );
    const outcome = runJob(channel, {}, { id });
    await expect(outcome).rejects.toBeInstanceOf(HestiaError);
    await outcome.catch((error: HestiaError) => {
      expect(error.info?.kind).toBe('already_exists');
    });
  });

  it('distinguishes a cancellation from a failure', async () => {
    const id = jobId(channel);
    onStart(() => later(() => publishJob(id, channel, { state: 'cancelled' })));
    await expect(runJob(channel, {}, { id })).rejects.toBeInstanceOf(
      JobCancelled,
    );
  });

  it('propagates a rejection from the start call itself', async () => {
    channels[channel] = () => {
      throw { code: 'bad_request', message: 'nope' };
    };
    await expect(
      runJob(channel, {}, { id: jobId(channel) }),
    ).rejects.toThrow('nope');
  });

  it('hears a terminal event that arrives before the start call returns', async () => {
    const id = jobId(channel);
    onStart(() =>
      publishJob(id, channel, { state: 'done', server, warnings: [], failures: [] }),
    );
    await expect(runJob(channel, {}, { id })).resolves.toMatchObject({
      server,
    });
  });
});
