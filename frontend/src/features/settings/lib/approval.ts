import type {
  Grant,
  Permission,
  PermissionScope,
  PluginInspection,
  Program,
  ProgramPin,
  ScopeKind,
} from '@/api/types';
import { m } from '@/paraglide/messages.js';

type GrantSectionKind =
  | 'full_access'
  | 'permissions'
  | 'contributions'
  | 'pinned';

/** One kind of entries a grant lists, under the permission that carries them. */
export interface GrantScope {
  heading: string;
  entries: string[];
}

/** One thing a grant allows: a permission and what it means, with its scope. */
export interface GrantItem {
  label: string;
  detail?: string;
  scopes: GrantScope[];
}

export interface DescribedSection {
  kind: GrantSectionKind;
  heading: string;
  items: GrantItem[];
  scopes: GrantScope[];
}

/**
 * A native message box has no scroll: GTK grows it past the screen and Windows
 * clips it. Past this many lines the dialog carries a summary and the page
 * carries the full list.
 */
const MAX_DIALOG_LINES = 12;

const PERMISSION_TEXT: Record<Permission, () => string> = {
  'entry:read': m['domain.plugin.permission.entry_read'],
  'entry:write': m['domain.plugin.permission.entry_write'],
  'process:read': m['domain.plugin.permission.process_read'],
  'process:control': m['domain.plugin.permission.process_control'],
  'process:spawn': m['domain.plugin.permission.process_spawn'],
  'content:read': m['domain.plugin.permission.content_read'],
  'content:write': m['domain.plugin.permission.content_write'],
  'account:read': m['domain.plugin.permission.account_read'],
  'config:read': m['domain.plugin.permission.config_read'],
  'config:write': m['domain.plugin.permission.config_write'],
  'java:read': m['domain.plugin.permission.java_read'],
  'java:manage': m['domain.plugin.permission.java_manage'],
  'network:connect': m['domain.plugin.permission.network_connect'],
  'ui:css': m['domain.plugin.permission.ui_css'],
  'launch:patch': m['domain.plugin.permission.launch_patch'],
  'ipc:connect': m['domain.plugin.permission.ipc_connect'],
  'flavor:provide': m['domain.plugin.permission.flavor_provide'],
  'java:provide': m['domain.plugin.permission.java_provide'],
  'pack:provide': m['domain.plugin.permission.pack_provide'],
};

const HEADING: Record<GrantSectionKind, () => string> = {
  full_access: m['settings.plugins.grant.full_access'],
  permissions: m['settings.plugins.grant.permissions'],
  contributions: m['settings.plugins.grant.contributions'],
  pinned: m['settings.plugins.grant.pinned'],
};

const KIND_HEADING: Record<ScopeKind, () => string> = {
  hosts: m['settings.plugins.grant.kind.hosts'],
  unhashed: m['settings.plugins.grant.kind.unhashed'],
  files: m['settings.plugins.grant.kind.files'],
  endpoints: m['settings.plugins.grant.kind.endpoints'],
  programs: m['settings.plugins.grant.kind.programs'],
  properties: m['settings.plugins.grant.kind.properties'],
  hooks: m['settings.plugins.grant.kind.hooks'],
  events: m['settings.plugins.grant.kind.events'],
  schedules: m['settings.plugins.grant.kind.schedules'],
  secrets: m['settings.plugins.grant.kind.secrets'],
  themes: m['settings.plugins.grant.kind.themes'],
};

const scopesOf = (scope: PermissionScope): GrantScope[] =>
  (Object.keys(KIND_HEADING) as ScopeKind[]).flatMap((kind) => {
    const entries = scope[kind] ?? [];
    return entries.length > 0
      ? [{ heading: KIND_HEADING[kind](), entries }]
      : [];
  });

const permissionItem = (
  name: Permission,
  scope: PermissionScope,
): GrantItem => ({
  label: name,
  detail: PERMISSION_TEXT[name](),
  scopes: scopesOf(scope),
});

/**
 * What `grant` allows, as non-empty sections in the order a person should weigh
 * them: the full-access permissions first (which ones those are is the
 * daemon's call, `unrestricted`), then the sandboxed ones, each with the scope
 * it carries, then what the plugin contributes and where its programs are
 * pinned.
 */
export function describeGrant(
  grant: Grant,
  unrestricted: Permission[],
  programPins: Record<string, ProgramPin> = {},
  unpinned: Program[] = [],
): DescribedSection[] {
  const full = new Set(unrestricted);
  const held = (Object.keys(grant.permissions) as Permission[]).map((name) =>
    permissionItem(name, grant.permissions[name] ?? {}),
  );
  const isFull = (item: GrantItem) => full.has(item.label as Permission);
  const pinned = [
    ...Object.entries(programPins).map(
      ([id, pin]) => `${id} → ${pin.path} (sha256 ${pin.sha256.slice(0, 12)})`,
    ),
    ...unpinned.map((program) =>
      m['settings.plugins.grant.unpinned']({
        id: program.id,
        command: program.command,
      }),
    ),
  ];
  const sections: DescribedSection[] = [
    {
      kind: 'full_access',
      heading: HEADING.full_access(),
      items: held.filter(isFull),
      scopes: [],
    },
    {
      kind: 'permissions',
      heading: HEADING.permissions(),
      items: held.filter((item) => !isFull(item)),
      scopes: [],
    },
    {
      kind: 'contributions',
      heading: HEADING.contributions(),
      items: [],
      scopes: scopesOf(grant.contributions),
    },
    {
      kind: 'pinned',
      heading: HEADING.pinned(),
      items: [],
      scopes: pinned.length > 0 ? [{ heading: '', entries: pinned }] : [],
    },
  ];
  return sections.filter(
    (section) => section.items.length > 0 || section.scopes.length > 0,
  );
}

/**
 * What an approval covers: the whole grant when new; on an update the
 * widening, plus every pinned program (path and content hash), since the digest
 * binds the pin and an update that only moves it must still show where it now
 * points, and every `PATH` program not found on this computer, which cannot run
 * until the plugin is approved again with it installed.
 */
export function describeApproval(
  inspection: PluginInspection,
): DescribedSection[] {
  const grant = inspection.replaces ? inspection.widened : inspection.grant;
  const unpinned = Object.values(inspection.grant.programs).filter(
    (program) =>
      program.command !== '' &&
      !program.command.includes('/') &&
      !(program.id in inspection.programPins),
  );
  return describeGrant(
    grant,
    inspection.unrestricted,
    inspection.programPins,
    unpinned,
  );
}

const scopeLines = (scopes: GrantScope[], indent: string): string[] =>
  scopes.flatMap(({ heading, entries }) => [
    ...(heading ? [`${indent}${heading}`] : []),
    ...entries.map((entry) => `${indent}  - ${entry}`),
  ]);

export interface ApprovalDialog {
  title: string;
  message: string;
  /** False when the message had to be cut short: the page must show the rest. */
  complete: boolean;
}

export function approvalDialog(inspection: PluginInspection): ApprovalDialog {
  const { manifest, replaces } = inspection;
  const name = manifest.name || manifest.id;
  const title = replaces
    ? m['settings.plugins.approval.update_title']({
        name,
        from: replaces.manifest.version,
        to: manifest.version,
      })
    : m['settings.plugins.approval.install_title']({
        name,
        version: manifest.version,
      });

  const sections = describeApproval(inspection);
  const lines: string[] = [];
  if (inspection.unrestricted.length > 0)
    lines.push(m['settings.plugins.approval.full_access_warning'](), '');
  if (sections.length === 0)
    lines.push(
      replaces
        ? m['settings.plugins.approval.nothing_new']()
        : m['settings.plugins.approval.none'](),
    );
  for (const section of sections) {
    lines.push(section.heading);
    for (const { label, detail, scopes } of section.items) {
      lines.push(detail ? `• ${label} — ${detail}` : `• ${label}`);
      lines.push(...scopeLines(scopes, '  '));
    }
    lines.push(...scopeLines(section.scopes, ''));
  }

  if (lines.length <= MAX_DIALOG_LINES)
    return { title, message: lines.join('\n'), complete: true };
  const kept = lines.slice(0, MAX_DIALOG_LINES - 2);
  kept.push('', m['settings.plugins.approval.more']());
  return { title, message: kept.join('\n'), complete: false };
}
