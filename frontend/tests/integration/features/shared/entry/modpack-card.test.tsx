import { screen, waitFor } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { InstalledModpack } from '@/api';
import { ModpackCard } from '@/features/shared/entry/detail';
import { clearPack, listInstances, setPack } from '@/mock/state/entries';
import { m } from '@/paraglide/messages.js';
import { renderWithProviders, resetQueryCache } from '../../../../support';

const instance = listInstances()[0];

const pack = (over: Partial<InstalledModpack> = {}): InstalledModpack =>
  ({
    source: 'modrinth',
    projectId: 'fabulously-optimized',
    slug: 'fabulously-optimized',
    name: 'Fabulously Optimized',
    versionId: 'v0',
    versionNumber: '1.4.0',
    gameVersion: '1.21.1',
    loader: 'fabric',
    loaderVersion: '0.16.5',
    installedUnix: 0,
    files: [],
    overrides: [],
    ...over,
  }) as InstalledModpack;

const card = (running: boolean) =>
  renderWithProviders(
    <ModpackCard
      kind="instance"
      id={instance.id}
      name={instance.name}
      flavor={instance.flavor}
      gameVersion={instance.gameVersion}
      running={running}
    />,
  );

const updateButton = () =>
  screen.getByRole('button', {
    name: m['content.modpack.update_to']({ version: '1.5.0' }),
  });
const removeItem = async () => {
  await userEvent.click(
    screen.getByRole('button', { name: m['app.action.more']() }),
  );
  return screen.findByRole('menuitem', {
    name: m['content.modpack.remove.action'](),
  });
};

beforeEach(() => setPack(instance.id, pack()));
afterEach(() => {
  clearPack(instance.id);
  resetQueryCache();
});

describe('the modpack card', () => {
  it('refuses both while the entry is running', async () => {
    card(true);
    await waitFor(() => expect(updateButton()).toBeDefined());
    expect(updateButton().hasAttribute('disabled')).toBe(true);
    expect((await removeItem()).hasAttribute('data-disabled')).toBe(true);
  });

  it('hides update for a pack with no catalogue behind it', async () => {
    setPack(instance.id, pack({ projectId: '' }));
    card(false);
    await screen.findByRole('button', {
      name: m['content.modpack.update_from_file.action'](),
    });
    expect(
      screen.queryByRole('button', { name: m['content.modpack.update']() }),
    ).toBeNull();
  });

  it('offers nothing to update when the pack is on the newest version', async () => {
    setPack(instance.id, pack({ versionNumber: '1.5.0' }));
    card(false);
    await waitFor(() =>
      expect(screen.getByText(m['content.modpack.up_to_date']())).toBeDefined(),
    );
    expect(
      screen.queryByRole('button', {
        name: m['content.modpack.update_to']({ version: '1.5.0' }),
      }),
    ).toBeNull();
  });
});
