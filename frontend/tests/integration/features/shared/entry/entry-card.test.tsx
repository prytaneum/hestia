import { screen, waitFor, within } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  type EntryCardModel,
  EntryTile,
} from '@/features/shared/entry/components';
import { m } from '@/paraglide/messages.js';
import { renderWithProviders, resetQueryCache } from '../../../../support';

const entry = (over: Partial<EntryCardModel> = {}): EntryCardModel => ({
  id: 'smp',
  name: 'SMP',
  kind: 'server',
  flavor: 'paper',
  version: '1.21.1',
  running: false,
  ready: true,
  subtitle: ':25565 · Stopped',
  ...over,
});

const show = (model: EntryCardModel) =>
  renderWithProviders(<EntryTile entry={model} view="grid" />, { route: true });

afterEach(resetQueryCache);

describe('EntryTile', () => {
  it('refuses to start an entry that is not provisioned yet', async () => {
    show(entry({ ready: false }));
    const button = await screen.findByRole('button', {
      name: m['app.action.start'](),
    });
    expect(button.hasAttribute('disabled')).toBe(true);
    expect(screen.getByText(m['app.status.preparing']())).toBeDefined();
  });

  it('refuses to start again while a start is in flight', async () => {
    show(entry({ busy: true }));
    const button = await screen.findByRole('button', {
      name: new RegExp(m['app.action.start']()),
    });
    expect(button.hasAttribute('disabled')).toBe(true);
    expect(screen.getByRole('status')).toBeDefined();
  });

  it('confirms before stopping a running entry', async () => {
    const onStop = vi.fn();
    show(entry({ running: true, onStop }));

    await userEvent.click(
      await screen.findByRole('button', { name: m['app.action.stop']() }),
    );
    expect(onStop).not.toHaveBeenCalled();

    const dialog = await screen.findByRole('alertdialog');
    await userEvent.click(
      within(dialog).getByRole('button', { name: m['app.action.stop']() }),
    );
    await waitFor(() => expect(onStop).toHaveBeenCalledOnce());
  });
});
