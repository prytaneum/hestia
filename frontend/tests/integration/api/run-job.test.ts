import { afterEach, describe, expect, it } from 'vitest';
import {
  HestiaError,
  JobCancelled,
  jobId,
  runJob,
} from '@/api';
import { publishJob } from '@/mock/bus';
import { channels } from '@/mock/channels';
import { listServers } from '@/mock/state/entries';
import { installFakeDaemon } from '../../support';

installFakeDaemon();

const server = listServers()[0];
const channel = 'server.create';
const fixture = channels[channel];

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
