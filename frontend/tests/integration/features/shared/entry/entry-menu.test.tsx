import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { server as serverApi } from '@/api';
import {
  type EntryCardModel,
  EntryTile,
} from '@/features/shared/entry/components';
import { listServers } from '@/mock/state/entries';
import { serverProcessId } from '@/lib/process-id';
import { isRunning } from '@/mock/state/processes';
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

async function openMenu(model: EntryCardModel) {
  renderWithProviders(<EntryTile entry={model} view="grid" />, { route: true });
  fireEvent.contextMenu(await screen.findByRole('link'));
  return screen.findByRole('menu');
}

const item = (menu: HTMLElement, name: string) =>
  within(menu).getByRole('menuitem', { name });

async function confirmDelete(menu: HTMLElement) {
  await userEvent.click(item(menu, m['app.action.delete']()));
  const dialog = await screen.findByRole('alertdialog');
  await userEvent.click(
    within(dialog).getByRole('button', {
      name: m['entry.settings.remove.server'](),
    }),
  );
}

afterEach(resetQueryCache);

describe('EntryContextMenu', () => {
  it('opens on right-click with the entry’s actions', async () => {
    const menu = await openMenu(entry());
    expect(item(menu, m['app.action.open_folder']())).toBeDefined();
    expect(item(menu, m['app.label.settings']())).toBeDefined();
    expect(item(menu, m['app.label.pin']())).toBeDefined();
    expect(item(menu, m['app.action.delete']())).toBeDefined();
    expect(
      within(menu).queryByRole('menuitem', { name: m['app.action.export']() }),
    ).toBeNull();
  });

  it('offers export on an instance only', async () => {
    const menu = await openMenu(
      entry({ kind: 'instance', id: 'kitchen-sink', name: 'Kitchen Sink' }),
    );
    expect(item(menu, m['app.action.export']())).toBeDefined();
  });

  it('flips the pin item once the entry is pinned', async () => {
    const menu = await openMenu(entry());
    await userEvent.click(item(menu, m['app.label.pin']()));

    fireEvent.contextMenu(screen.getByRole('link'));
    const reopened = await screen.findByRole('menu');
    expect(item(reopened, m['app.label.unpin']())).toBeDefined();
  });

  it('removes a stopped entry once confirmed', async () => {
    const menu = await openMenu(entry({ id: 'creative', name: 'Creative' }));
    await userEvent.click(item(menu, m['app.action.delete']()));
    expect(listServers().some((s) => s.id === 'creative')).toBe(true);

    const dialog = await screen.findByRole('alertdialog');
    expect(
      within(dialog).queryByText(m['entry.remove.stops_first'](), {
        exact: false,
      }),
    ).toBeNull();
    await userEvent.click(
      within(dialog).getByRole('button', {
        name: m['entry.settings.remove.server'](),
      }),
    );
    await waitFor(() =>
      expect(listServers().some((s) => s.id === 'creative')).toBe(false),
    );
  });

  it('stops a running entry before removing it', async () => {
    await serverApi.start('smp');
    expect(isRunning(serverProcessId('smp'))).toBe(true);

    const menu = await openMenu(entry({ running: true }));
    await confirmDelete(menu);

    await waitFor(() =>
      expect(listServers().some((s) => s.id === 'smp')).toBe(false),
    );
    expect(isRunning(serverProcessId('smp'))).toBe(false);
  });
});
