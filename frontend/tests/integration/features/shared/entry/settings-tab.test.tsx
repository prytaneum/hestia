import { screen, waitFor } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ConfigEntry } from '@/api';
import { InstanceSettingsTab } from '@/features/instances/tabs';
import { ServerSettingsTab } from '@/features/servers/tabs';
import { listInstances, listServers } from '@/mock/state/entries';
import { m } from '@/paraglide/messages.js';
import { renderWithProviders, resetQueryCache } from '../../../../support';

const server = listServers()[0];
const instance = listInstances()[0];

const config = (over: Record<string, string> = {}): ConfigEntry[] =>
  Object.entries({
    memory: '6G',
    'jvm-args': '-XX:+UseZGC',
    'backup-interval': '',
    'backup-retention': '',
    motd: 'A Minecraft Server',
    pvp: 'true',
    'server-port': '25565',
    ...over,
  }).map(([key, value]) => ({ key, value }));

const showServer = (running = false) =>
  renderWithProviders(
    <ServerSettingsTab server={server} config={config()} running={running} />,
    { route: true },
  );

const showInstance = (running = false) =>
  renderWithProviders(
    <InstanceSettingsTab
      instance={instance}
      config={config()}
      running={running}
    />,
    { route: true },
  );

const open = async (section: string) =>
  userEvent.click(await screen.findByRole('tab', { name: section }));

const button = (name: string) => screen.getByRole('button', { name });

afterEach(() => {
  resetQueryCache();
  vi.restoreAllMocks();
});

describe('the general section', () => {
  it('seeds the name from the entry', async () => {
    showServer();
    const name = (await screen.findByLabelText(
      m['entry.settings.server_name'](),
    )) as HTMLInputElement;
    expect(name.value).toBe(server.name);
  });

  it('labels the name field for the kind it is showing', async () => {
    showInstance();
    expect(
      await screen.findByLabelText(m['entry.settings.instance_name']()),
    ).toBeDefined();
    expect(
      screen.queryByLabelText(m['entry.settings.server_name']()),
    ).toBeNull();
  });

  it('keeps rename inert until the name actually changes', async () => {
    showServer();
    const name = await screen.findByLabelText(
      m['entry.settings.server_name'](),
    );
    const apply = button(m['app.action.apply']());
    expect(apply.hasAttribute('disabled')).toBe(true);

    await userEvent.type(name, '-renamed');
    expect(apply.hasAttribute('disabled')).toBe(false);
  });

  it('locks the identity controls while the entry runs', async () => {
    showServer(true);
    const name = await screen.findByLabelText(
      m['entry.settings.server_name'](),
    );
    expect(name.hasAttribute('disabled')).toBe(true);
    expect(
      button(m['entry.settings.change_version']()).hasAttribute('disabled'),
    ).toBe(true);
  });

  it('opens the version dialog', async () => {
    showServer();
    await userEvent.click(
      await screen.findByRole('button', {
        name: m['entry.settings.change_version'](),
      }),
    );
    const dialog = await screen.findByRole('dialog');
    expect(dialog.textContent).toContain(m['entry.settings.allow_downgrade']());
  });
});

describe('the java section', () => {
  it('saves an edit and stays inert without one', async () => {
    showServer();
    await open(m['app.label.java']());
    const args = (await screen.findByLabelText(
      m['entry.settings.java_arguments'](),
    )) as HTMLInputElement;
    expect(args.value).toBe('-XX:+UseZGC');

    const save = button(m['app.action.save']());
    expect(save.hasAttribute('disabled')).toBe(true);

    await userEvent.clear(args);
    await userEvent.type(args, '-Xshare:off');
    expect(screen.getByText(m['entry.settings.unsaved']())).toBeDefined();
    await userEvent.click(save);

    await waitFor(() =>
      expect(screen.getByText(m['app.toast.saved']())).toBeDefined(),
    );
  });

  it('puts an edit back on discard', async () => {
    showServer();
    await open(m['app.label.java']());
    const args = (await screen.findByLabelText(
      m['entry.settings.java_arguments'](),
    )) as HTMLInputElement;
    await userEvent.type(args, ' -Xshare:off');
    await userEvent.click(button(m['entry.settings.discard']()));
    expect(args.value).toBe('-XX:+UseZGC');
  });
});

describe('the game properties section', () => {
  it('lists the properties file and none of the record settings', async () => {
    showServer();
    await open(m['server.properties.title']());
    expect(await screen.findByLabelText(/^motd/)).toBeDefined();
    expect(screen.queryByLabelText(/^jvm-args/)).toBeNull();
  });

  it('holds a managed key read-only', async () => {
    showServer();
    await open(m['server.properties.title']());
    const port = await screen.findByLabelText(/^server-port/);
    expect(port.hasAttribute('disabled')).toBe(true);
  });

  it('narrows the list as the search is typed', async () => {
    showServer();
    await open(m['server.properties.title']());
    await userEvent.type(
      await screen.findByPlaceholderText(m['server.properties.search']()),
      'pvp',
    );
    await waitFor(() => expect(screen.queryByLabelText(/^motd/)).toBeNull());
    expect(screen.getByRole('switch', { name: /^pvp/ })).toBeDefined();
  });

  it('saves an edited property', async () => {
    showServer();
    await open(m['server.properties.title']());
    await userEvent.type(await screen.findByLabelText(/^motd/), '!');
    await userEvent.click(button(m['app.action.save']()));
    await waitFor(() =>
      expect(screen.getByText(m['app.toast.saved']())).toBeDefined(),
    );
  });

  it('is not offered on an instance', async () => {
    showInstance();
    await screen.findByLabelText(m['entry.settings.instance_name']());
    expect(
      screen.queryByRole('tab', { name: m['server.properties.title']() }),
    ).toBeNull();
  });
});

describe('the backups section', () => {
  it('offers the schedule on a server', async () => {
    showServer();
    await open(m['app.label.backups']());
    expect(
      await screen.findByLabelText(m['entry.settings.keep_backups']()),
    ).toBeDefined();
  });

  it('is not offered on an instance', async () => {
    showInstance();
    await screen.findByLabelText(m['entry.settings.instance_name']());
    expect(
      screen.queryByRole('tab', { name: m['app.label.backups']() }),
    ).toBeNull();
  });
});

describe('the danger zone', () => {
  it('names removal after the server it would remove', async () => {
    showServer();
    await open(m['entry.settings.section.danger']());
    expect(
      await screen.findByRole('button', {
        name: m['entry.settings.remove.server'](),
      }),
    ).toBeDefined();
  });

  it('names removal after the instance it would remove', async () => {
    showInstance();
    await open(m['entry.settings.section.danger']());
    expect(
      await screen.findByRole('button', {
        name: m['entry.settings.remove.instance'](),
      }),
    ).toBeDefined();
  });

  it('locks removal while the entry runs', async () => {
    showServer(true);
    await open(m['entry.settings.section.danger']());
    const remove = await screen.findByRole('button', {
      name: m['entry.settings.remove.server'](),
    });
    expect(remove.hasAttribute('disabled')).toBe(true);
  });
});
