import { screen } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ConfigEntry } from '@/api';
import { InstanceSettingsTab } from '@/features/instances/tabs';
import { PropertiesSettings } from '@/features/servers/settings';
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
    <PropertiesSettings id={server.id} running={running} />,
    { route: true },
  );

const open = async (tab: string) =>
  userEvent.click(await screen.findByRole('tab', { name: tab }));

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

    await open(m['app.label.java']());
    const args = (await screen.findByLabelText(
      m['entry.settings.java_arguments'](),
    )) as HTMLInputElement;
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
    await open(m['app.label.java']());
    const args = await screen.findByLabelText(
      m['entry.settings.java_arguments'](),
    );
    await userEvent.clear(args);
    await userEvent.type(args, '-Xshare:off{Enter}');

    expect(await screen.findAllByText(m['app.status.saved']())).not.toHaveLength(0);
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
  it('holds a managed key read-only', async () => {
    showProperties();
    const port = await screen.findByLabelText(/^server-port/);
    expect(port.hasAttribute('disabled')).toBe(true);
  });

  it('saves a typed property and confirms it', async () => {
    showProperties();
    await userEvent.type(await screen.findByLabelText(/^motd/), '!{Enter}');
    expect(await screen.findAllByText(m['app.status.saved']())).not.toHaveLength(0);
  });
});
