import { screen } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ErrorBoundary } from '@/components/error-boundary';
import { m } from '@/paraglide/messages.js';
import { renderWithProviders, resetQueryCache } from '../../support';

function Boom({ throws }: { throws: boolean }): React.ReactNode {
  if (throws) throw new Error('the sky fell');
  return <p>all good</p>;
}

beforeEach(() => {
  // React logs a caught render error; the boundary is the assertion here.
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => {
  resetQueryCache();
  vi.restoreAllMocks();
});

describe('ErrorBoundary', () => {
  it('shows the error’s own message so a report can be matched to it', () => {
    renderWithProviders(
      <ErrorBoundary>
        <Boom throws />
      </ErrorBoundary>,
    );
    expect(screen.getByText('the sky fell')).toBeDefined();
  });

  it('retries the subtree when asked', async () => {
    let throws = true;
    function Subject() {
      return <Boom throws={throws} />;
    }

    renderWithProviders(
      <ErrorBoundary>
        <Subject />
      </ErrorBoundary>,
    );
    expect(screen.getByText(m['app.crash.title']())).toBeDefined();

    throws = false;
    await userEvent.click(
      screen.getByRole('button', { name: m['app.crash.retry']() }),
    );
    expect(screen.getByText('all good')).toBeDefined();
  });
});
