import { screen, waitFor, within } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { ServerFilesTab } from '@/features/servers/tabs';
import { m } from '@/paraglide/messages.js';
import { renderWithProviders, resetQueryCache } from '../../../support';

// Each test takes its own server, so one test's writes never reach another's
// listing: the fixture tree is per server and seeded on first use.
let serial = 0;

function show(path = '') {
  const onNavigate = vi.fn();
  const onOpenFile = vi.fn();
  serial += 1;
  renderWithProviders(
    <ServerFilesTab
      id={`files-${serial}`}
      running={false}
      path={path}
      onNavigate={onNavigate}
      onOpenFile={onOpenFile}
    />,
    { route: true },
  );
  return { onNavigate, onOpenFile };
}

const row = (name: RegExp) => screen.findByRole('button', { name });

afterEach(() => {
  resetQueryCache();
  vi.restoreAllMocks();
});

describe('the server files tab', () => {
  it('creates a folder and lists it', async () => {
    show();
    await userEvent.click(
      await screen.findByRole('button', {
        name: m['server.files.new_folder'](),
      }),
    );
    const dialog = await screen.findByRole('dialog');
    await userEvent.type(
      within(dialog).getByLabelText(m['app.label.name']()),
      'datapacks',
    );
    await userEvent.click(
      within(dialog).getByRole('button', { name: m['server.files.create']() }),
    );
    expect(await row(/^datapacks/)).toBeDefined();
  });

  it('deletes what was selected, after confirming', async () => {
    show();
    await row(/^eula\.txt/);
    await userEvent.click(
      screen.getByRole('button', { name: m['app.action.select']() }),
    );
    await userEvent.click(screen.getByRole('checkbox', { name: /^eula\.txt/ }));
    await userEvent.click(
      screen.getByRole('button', {
        name: m['server.files.delete_count']({ count: 1 }),
      }),
    );
    const confirm = await screen.findByRole('alertdialog');
    await userEvent.click(
      within(confirm).getByRole('button', { name: m['app.action.delete']() }),
    );
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: /^eula\.txt/ })).toBeNull(),
    );
  });
});
