import { describe, expect, it } from 'vitest';
import type { Grant, PermissionScope, ScopeKind } from '@/api/types';
import { describeGrant } from '@/features/settings/lib';

const ENTRY: Record<ScopeKind, string> = {
  hosts: 'api.example.com',
  unhashed: 'files.example.com',
  files: 'ops.json',
  endpoints: 'discord-ipc-*',
  programs: 'git: git args ["status"]',
  properties: 'my.flag',
  hooks: 'before-launch',
  events: 'server.*',
  schedules: 'poll: 300s',
  secrets: 'token',
  themes: 'ember',
};

const KINDS = Object.keys(ENTRY) as ScopeKind[];

const one = (kind: ScopeKind): PermissionScope => ({ [kind]: [ENTRY[kind]] });

const carried: Grant = {
  permissions: {
    'network:connect': { ...one('hosts'), ...one('unhashed') },
    'entry:read': one('files'),
    'ipc:connect': one('endpoints'),
    'process:spawn': one('programs'),
    'launch:patch': one('properties'),
  },
  contributions: {
    ...one('hooks'),
    ...one('events'),
    ...one('schedules'),
    ...one('secrets'),
    ...one('themes'),
  },
  programs: {},
};

const shown = (sections: ReturnType<typeof describeGrant>) =>
  sections.flatMap((section) => [
    ...section.scopes.flatMap((scope) => scope.entries),
    ...section.items.flatMap((item) =>
      item.scopes.flatMap((scope) => scope.entries),
    ),
  ]);

describe('describeGrant', () => {
  it('renders every scope kind, under its permission or as a contribution', () => {
    const entries = shown(describeGrant(carried, ['ipc:connect']));
    for (const kind of KINDS) expect(entries).toContain(ENTRY[kind]);
  });

  it('puts a permission with its scope, full access first', () => {
    const [full, sandboxed] = describeGrant(carried, [
      'ipc:connect',
      'process:spawn',
    ]);
    expect(full.kind).toBe('full_access');
    expect(full.items.map((item) => item.label)).toEqual([
      'ipc:connect',
      'process:spawn',
    ]);
    expect(full.items[0].scopes[0].entries).toEqual(['discord-ipc-*']);
    expect(sandboxed.kind).toBe('permissions');
    const network = sandboxed.items.find(
      (item) => item.label === 'network:connect',
    );
    expect(network?.scopes.flatMap((scope) => scope.entries)).toEqual([
      'api.example.com',
      'files.example.com',
    ]);
  });

  it('shows a widening the same way: an added entry under its permission', () => {
    const widening: Grant = {
      permissions: { 'entry:read': { files: ['whitelist.json'] } },
      contributions: {},
      programs: {},
    };
    const sections = describeGrant(widening, []);
    expect(sections).toHaveLength(1);
    expect(sections[0].items[0].label).toBe('entry:read');
    expect(shown(sections)).toEqual(['whitelist.json']);
  });

  it('lists the pinned program paths and nothing for an empty grant', () => {
    const empty: Grant = { permissions: {}, contributions: {}, programs: {} };
    expect(describeGrant(empty, [])).toEqual([]);
    expect(shown(describeGrant(empty, [], { git: '/usr/bin/git' }))).toEqual([
      'git → /usr/bin/git',
    ]);
  });
});
