/**
 * `plugin.*` — a few installed plugins, and an inspection that asks for
 * enough to exercise the approval dialog.
 */
import type {
  Grant,
  Manifest,
  PluginInfo,
  PluginPage,
  PluginState,
  Setting,
  UiNode,
} from '@/api/types';

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
  ipc: [],
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
  ipc: [],
  programs: [],
  launchProperties: [],
  hooks: [],
  events: [],
  secretSettings: [],
  uiCss: [],
};

interface Fixture {
  id: string;
  name: string;
  state: PluginState;
  description?: string;
  authors?: string[];
  grant?: Grant;
  unrestricted?: Grant['permissions'];
  commands?: number;
  themes?: number;
  settings?: Setting[];
  lastError?: string;
  stats?: { calls: number; traps: number; timeouts: number };
}

const installed = ({
  id,
  name,
  state,
  description = '',
  authors = [],
  grant = emptyGrant,
  unrestricted = [],
  commands = 0,
  themes = 0,
  settings = [],
  lastError,
  stats = { calls: 0, traps: 0, timeouts: 0 },
}: Fixture): PluginInfo => {
  const base = manifest(id, name);
  return {
    manifest: {
      ...base,
      description,
      authors,
      permissions: grant.permissions,
      network: grant.network,
      settings,
      contributes: {
        ...base.contributes,
        commands: Array.from({ length: commands }, (_, i) => ({
          id: `command-${i}`,
          title: `Command ${i}`,
          description: '',
          scope: 'top' as const,
          args: [],
        })),
        themes: Array.from({ length: themes }, (_, i) => ({
          id: `theme-${i}`,
          name: `Theme ${i}`,
          appearance: 'dark' as const,
          file: `themes/${i}.css`,
        })),
      },
    },
    state,
    source: 'file',
    sha256: '9f2c4e81a7d05b3c6e1f8a2d4b7c9e0f1a3d5b7c9e2f4a6b8d0c1e3f5a7b9d2c',
    grant,
    unrestricted,
    obsolete: false,
    official: false,
    calls: stats.calls,
    traps: stats.traps,
    timeouts: stats.timeouts,
    ...(lastError ? { lastError } : {}),
  };
};

const plugins: PluginInfo[] = [
  installed({
    id: 'hello',
    name: 'Hello',
    state: 'enabled',
    description: 'Greets you in the log every time the daemon starts.',
    authors: ['Hestia'],
    grant: { ...emptyGrant, permissions: ['entry:read'] },
    commands: 1,
    stats: { calls: 128, traps: 0, timeouts: 0 },
  }),
  installed({
    id: 'backup-plus',
    name: 'Backup Plus',
    state: 'enabled',
    settings: [
      {
        key: 'access-key',
        kind: 'secret',
        default: null,
        label: 'Access key',
        help: 'Used to sign uploads.',
        options: [],
      },
      {
        key: 'keep',
        kind: 'number',
        default: 10,
        label: 'Backups to keep',
        help: '',
        options: [],
      },
      {
        key: 'verbose',
        kind: 'bool',
        default: null,
        label: 'Log every upload',
        help: '',
        options: [],
      },
      {
        key: 'region',
        kind: 'select',
        default: 'eu',
        label: 'Region',
        help: '',
        options: ['eu', 'us'],
      },
    ],
    description:
      'Uploads server backups to S3-compatible storage after every scheduled backup, and prunes old ones.',
    authors: ['Ana Ribeiro', 'kodo'],
    grant: {
      ...emptyGrant,
      permissions: ['entry:read', 'entry:write', 'network:connect'],
      network: ['s3.amazonaws.com', 'r2.cloudflarestorage.com'],
    },
    unrestricted: ['entry:write'],
    commands: 2,
    stats: { calls: 3041, traps: 1, timeouts: 2 },
  }),
  installed({
    id: 'ember-night',
    name: 'Ember Night',
    state: 'disabled',
    description: 'A warm dark theme.',
    authors: ['Hestia'],
    grant: { ...emptyGrant, uiCss: ['themes/0.css'] },
    themes: 1,
  }),
  installed({
    id: 'discord-status',
    name: 'Discord Status',
    state: 'failed',
    description: 'Shows what you are playing in Discord.',
    authors: ['someone'],
    grant: { ...emptyGrant, permissions: ['process:read'] },
    lastError: 'disabled after repeated traps',
    stats: { calls: 17, traps: 3, timeouts: 0 },
  }),
  installed({
    id: 'mod-sync',
    name: 'Mod Sync',
    state: 'needs_approval',
    description: 'Keeps the mods of several instances in step.',
    authors: ['kodo'],
    grant: { ...emptyGrant, permissions: ['content:read', 'content:write'] },
    unrestricted: ['content:write'],
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

const stored = new Map<string, unknown>();

const find = (id: string) =>
  plugins.find((plugin) => plugin.manifest.id === id);

const setState = (id: string, state: PluginState): PluginInfo => {
  const plugin = find(id);
  if (!plugin) throw new Error(`no plugin ${id}`);
  plugin.state = state;
  return plugin;
};

const pages: PluginPage[] = [
  {
    plugin: 'hello',
    page: { id: 'greeter', title: 'Greeter', icon: 'plugin', slot: 'nav' },
  },
  {
    plugin: 'hello',
    page: {
      id: 'notes',
      title: 'Notes',
      icon: 'server',
      slot: 'entry',
      kind: 'server',
    },
  },
];

const greeter = {
  type: 'stack',
  direction: 'vertical',
  children: [
    { type: 'heading', text: 'Hello', level: 1 },
    { type: 'markdown', source: 'Say **hi** to the daemon.' },
    {
      type: 'form',
      id: 'greet',
      fields: [{ type: 'text', name: 'who', label: 'Who', value: 'world' }],
      submit: { label: 'Greet', action: { id: 'greet', payload: null } },
    },
  ],
} as unknown as UiNode;

export const channels: ChannelHandlers = {
  'plugin.pages': () => pages,
  'plugin.themes': () => [],
  'plugin.ui.render': () => greeter,
  'plugin.ui.action': () => [
    { type: 'toast', tone: 'success', text: 'Hello from the mock plugin' },
  ],
  'plugin.list': () => plugins,
  'plugin.settings.get': (p) =>
    find(str(p, 'id'))?.manifest.settings.map((setting) => ({
      key: setting.key,
      set: stored.has(setting.key),
      value:
        setting.kind === 'secret'
          ? undefined
          : (stored.get(setting.key) ?? setting.default),
    })) ?? [],
  'plugin.settings.set': (p) => {
    if (p.value === null) stored.delete(str(p, 'key'));
    else stored.set(str(p, 'key'), p.value);
    return ok();
  },
  'plugin.enable': (p) => setState(str(p, 'id'), 'enabled'),
  'plugin.disable': (p) => setState(str(p, 'id'), 'disabled'),
  'plugin.remove': (p) => {
    const at = plugins.findIndex((plugin) => plugin.manifest.id === p.id);
    if (at >= 0) plugins.splice(at, 1);
    return ok();
  },
  'plugin.inspect': (p) => ({
    ...(p.url ? { staged: 'mock-staged' } : {}),
    manifest: {
      ...manifest('new-plugin', 'New plugin'),
      permissions: requested.permissions,
    },
    grant: requested,
    grantDigest: 'mock-digest',
    widened: requested,
    unrestricted: ['entry:write', 'process:control'],
    programPaths: {},
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
        const plugin = installed({
          id: 'new-plugin',
          name: 'New plugin',
          state: 'enabled',
          grant: requested,
          unrestricted: ['entry:write', 'process:control'],
        });
        plugins.push(plugin);
        return { plugin, warnings: [] };
      },
    }),
};
