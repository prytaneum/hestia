/**
 * `plugin.*` — a few installed plugins, and an inspection that asks for
 * enough to exercise the approval dialog.
 */
import type { Grant, Manifest, PluginInfo, PluginState } from '@/api/types';

import { jobIdOf, runPlan } from '../job';
import { type ChannelHandlers, ok, str } from '../support';

const manifest = (id: string, name: string): Manifest => ({
  id,
  name,
  version: '1.0.0',
  api: '^1.0.0',
  description: '',
  authors: [],
  permissions: [],
  network: [],
  programs: [],
  launch: { properties: [] },
  settings: [],
  contributes: {
    commands: [],
    events: [],
    hooks: [],
    providers: [],
    pages: [],
    themes: [],
  },
});

const emptyGrant: Grant = {
  permissions: [],
  network: [],
  programs: [],
  launchProperties: [],
  hooks: [],
  events: [],
  secretSettings: [],
  uiCss: [],
};

const installed = (
  id: string,
  name: string,
  state: PluginState,
  grant: Grant = emptyGrant,
): PluginInfo => ({
  manifest: { ...manifest(id, name), permissions: grant.permissions },
  state,
  source: 'file',
  sha256: '',
  grant,
  calls: 0,
  traps: 0,
  timeouts: 0,
});

const plugins: PluginInfo[] = [
  installed('hello', 'Hello', 'enabled', {
    ...emptyGrant,
    permissions: ['entry:read'],
  }),
  installed('backup-plus', 'Backup Plus', 'disabled', {
    ...emptyGrant,
    permissions: ['entry:read', 'entry:write'],
  }),
];

const requested: Grant = {
  ...emptyGrant,
  permissions: [
    'entry:read',
    'entry:write',
    'process:control',
    'network:connect',
  ],
  network: ['api.example.com'],
};

const find = (id: string) =>
  plugins.find((plugin) => plugin.manifest.id === id);

const setState = (id: string, state: PluginState): PluginInfo => {
  const plugin = find(id);
  if (!plugin) throw new Error(`no plugin ${id}`);
  plugin.state = state;
  return plugin;
};

export const channels: ChannelHandlers = {
  'plugin.list': () => plugins,
  'plugin.enable': (p) => setState(str(p, 'id'), 'enabled'),
  'plugin.disable': (p) => setState(str(p, 'id'), 'disabled'),
  'plugin.remove': (p) => {
    const at = plugins.findIndex((plugin) => plugin.manifest.id === p.id);
    if (at >= 0) plugins.splice(at, 1);
    return ok();
  },
  'plugin.inspect': () => ({
    manifest: {
      ...manifest('new-plugin', 'New plugin'),
      permissions: requested.permissions,
    },
    grant: requested,
    grantDigest: 'mock-digest',
    widened: requested,
    unrestricted: ['entry:write', 'process:control'],
  }),
  'plugin.install': (p) =>
    runPlan({
      id: jobIdOf(p, 'plugin-install'),
      channel: 'plugin.install',
      ticks: [
        { phase: 'extracting' },
        { phase: 'verifying' },
        { phase: 'installing' },
      ],
      done: () => {
        const plugin = installed(
          'new-plugin',
          'New plugin',
          'enabled',
          requested,
        );
        plugins.push(plugin);
        return { plugin };
      },
    }),
};
