import { screen, waitFor } from '@testing-library/react';
import { act } from 'react';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { publishJob } from '@/mock/bus';
import { jobMutation, startJobs, useJobMutation } from '@/queries/jobs';
import {
  installFakeDaemon,
  renderWithProviders,
  resetJobs,
  resetQueryCache,
} from '../../support';

beforeAll(async () => {
  installFakeDaemon();
  await startJobs();
});

afterEach(async () => {
  await resetJobs();
  resetQueryCache();
});

const received: string[] = [];
let finish: ((value: string) => void) | null = null;

const spec = jobMutation<string, void>({
  mutationKey: ['test', 'job'],
  meta: () => ({ kind: 'server.create', label: 'create' }),
  run: (_variables, job) => {
    received.push(job.id);
    return new Promise<string>((resolve) => {
      finish = resolve;
    });
  },
});

function Harness() {
  const run = useJobMutation(spec);
  return (
    <div>
      <button type="button" onClick={() => run.mutate()}>
        go
      </button>
      <span data-testid="id">{run.job?.id ?? '-'}</span>
      <span data-testid="progress">{run.progress?.current ?? '-'}</span>
      <span data-testid="status">{run.job?.status ?? '-'}</span>
    </div>
  );
}

describe('useJobMutation', () => {
  it('tracks the run it started and streams its progress', async () => {
    renderWithProviders(<Harness />);
    screen.getByText('go').click();

    await waitFor(() => expect(received).toHaveLength(1));
    await waitFor(() =>
      expect(screen.getByTestId('id').textContent).toBe(received[0]),
    );

    act(() =>
      publishJob(received[0], 'server.create', {
        state: 'progress',
        phase: 'resolving',
        current: 42,
        total: 100,
        detail: '',
        item: 0,
        items: 0,
      }),
    );
    await waitFor(() =>
      expect(screen.getByTestId('progress').textContent).toBe('42'),
    );

    await act(async () => {
      finish?.('done');
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(screen.getByTestId('status').textContent).toBe('done'),
    );
  });
});
