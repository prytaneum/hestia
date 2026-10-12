import { screen, waitFor } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ConfigEntry } from '@/api';
import { InstanceSettingsTab } from '@/features/instances/tabs';
import { ServerPropertiesTab, ServerSettingsTab } from '@/features/servers/tabs';
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

const showProperties = (running = false) =>
  renderWithProviders(
    <ServerPropertiesTab id={server.id} running={running} />,
    { route: true },
  );

const save = () => screen.queryByRole('button', { name: m['app.action.save']() });

afterEach(() => {
  resetQueryCache();
  vi.restoreAllMocks();
});

describe('the shared entry settings tab', () => {
  it('seeds the name and JVM args from the entry and its config', async () => {
    showServer();
    const name = (await screen.findByLabelText(
      m['entry.settings.server_name'](),
    )) as HTMLInputElement;
    expect(name.value).toBe(server.name);

    const args = screen.getByLabelText(
      m['entry.settings.java_arguments'](),
    ) as HTMLInputElement;
    expect(args.value).toBe('-XX:+UseZGC');
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

  it('offers Save only once a typed value differs', async () => {
    showServer();
    const name = await screen.findByLabelText(
      m['entry.settings.server_name'](),
    );
    expect(save()).toBeNull();

    await userEvent.type(name, '-renamed');
    expect(save()).not.toBeNull();
  });

  it('saves the JVM args and confirms it', async () => {
    showServer();
    const args = await screen.findByLabelText(
      m['entry.settings.java_arguments'](),
    );
    await userEvent.clear(args);
    await userEvent.type(args, '-Xshare:off{Enter}');

    await waitFor(() =>
      expect(screen.getByText(m['app.toast.saved']())).toBeDefined(),
    );
  });

  it('locks the destructive and identity controls while the entry runs', async () => {
    showServer(true);
    const name = await screen.findByLabelText(
      m['entry.settings.server_name'](),
    );
    expect(name.hasAttribute('disabled')).toBe(true);
    expect(
      screen
        .getByRole('button', { name: m['entry.settings.change_version']() })
        .hasAttribute('disabled'),
    ).toBe(true);
    expect(
      screen
        .getByRole('button', { name: m['app.action.remove']() })
        .hasAttribute('disabled'),
    ).toBe(true);
  });

  it('offers the backup schedule on a server', async () => {
    showServer();
    expect(
      await screen.findByLabelText(m['entry.settings.keep_backups']()),
    ).toBeDefined();
  });

  it('has no backup schedule on an instance', async () => {
    showInstance();
    await screen.findByLabelText(m['entry.settings.instance_name']());
    expect(
      screen.queryByLabelText(m['entry.settings.keep_backups']()),
    ).toBeNull();
  });

  it('names removal after the server it would remove', async () => {
    showServer();
    expect(
      await screen.findByText(m['entry.settings.remove.server']()),
    ).toBeDefined();
  });

  it('names removal after the instance it would remove', async () => {
    showInstance();
    expect(
      await screen.findByText(m['entry.settings.remove.instance']()),
    ).toBeDefined();
  });

  it('opens the version dialog from the tab', async () => {
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

describe('the server properties tab', () => {
  it('lists the properties file and none of the record settings', async () => {
    showProperties();
    expect(await screen.findByLabelText(/^motd/)).toBeDefined();
    expect(screen.queryByLabelText(/^jvm-args/)).toBeNull();
  });

  it('holds a managed key read-only', async () => {
    showProperties();
    const port = await screen.findByLabelText(/^server-port/);
    expect(port.hasAttribute('disabled')).toBe(true);
  });

  it('narrows the list as the search is typed', async () => {
    showProperties();
    await userEvent.type(
      await screen.findByPlaceholderText(m['server.properties.search']()),
      'pvp',
    );
    await waitFor(() => expect(screen.queryByLabelText(/^motd/)).toBeNull());
    expect(screen.getByRole('switch', { name: /^pvp/ })).toBeDefined();
  });

  it('saves a typed property and confirms it', async () => {
    showProperties();
    await userEvent.type(await screen.findByLabelText(/^motd/), '!{Enter}');
    await waitFor(() =>
      expect(screen.getByText(m['app.toast.saved']())).toBeDefined(),
    );
  });

  it('says when a change waits for the next start', async () => {
    showProperties(true);
    expect(
      await screen.findByText(m['entry.settings.applies_on_start'](), {
        exact: false,
      }),
    ).toBeDefined();
  });
});
