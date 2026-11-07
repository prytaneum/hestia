import type { Grant, Permission, PluginInspection } from '@/api/types';
import { m } from '@/paraglide/messages.js';

type GrantSectionKind =
  | 'full_access'
  | 'permissions'
  | 'programs'
  | 'network'
  | 'launch_properties'
  | 'hooks'
  | 'events'
  | 'secret_settings'
  | 'themes';

/** One thing a grant allows: a permission and what it means, or a listed value. */
export interface GrantItem {
  label: string;
  detail?: string;
}

export interface DescribedSection {
  kind: GrantSectionKind;
  heading: string;
  items: GrantItem[];
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
};

const HEADING: Record<GrantSectionKind, () => string> = {
  full_access: m['settings.plugins.grant.full_access'],
  permissions: m['settings.plugins.grant.permissions'],
  programs: m['settings.plugins.grant.programs'],
  network: m['settings.plugins.grant.network'],
  launch_properties: m['settings.plugins.grant.launch_properties'],
  hooks: m['settings.plugins.grant.hooks'],
  events: m['settings.plugins.grant.events'],
  secret_settings: m['settings.plugins.grant.secret_settings'],
  themes: m['settings.plugins.grant.themes'],
};

const permission = (name: Permission): GrantItem => ({
  label: name,
  detail: PERMISSION_TEXT[name](),
});

const values = (list: string[]): GrantItem[] =>
  list.map((label) => ({ label }));

/**
 * What `grant` allows, as non-empty sections in the order a person should weigh
 * them: the full-access permissions first (which ones those are is the daemon's
 * call, `unrestricted`), then the sandboxed ones, then the narrower lists.
 */
export function describeGrant(
  grant: Grant,
  unrestricted: Permission[],
  programPaths: Record<string, string> = {},
): DescribedSection[] {
  const full = new Set(unrestricted);
  const sections: Array<[GrantSectionKind, GrantItem[]]> = [
    ['full_access', unrestricted.map(permission)],
    [
      'permissions',
      grant.permissions.filter((name) => !full.has(name)).map(permission),
    ],
    [
      'programs',
      grant.programs.map((program) => ({
        label: program.id,
        detail: [
          [program.command, ...program.args].join(' '),
          program.env.length > 0 &&
            m['settings.plugins.grant.program_env']({
              names: program.env.join(', '),
            }),
          programPaths[program.id] &&
            m['settings.plugins.grant.program_path']({
              path: programPaths[program.id],
            }),
        ]
          .filter(Boolean)
          .join(' · '),
      })),
    ],
    ['network', values(grant.network)],
    ['launch_properties', values(grant.launchProperties)],
    ['hooks', values(grant.hooks)],
    ['events', values(grant.events)],
    ['secret_settings', values(grant.secretSettings)],
    ['themes', values(grant.uiCss)],
  ];
  return sections
    .filter(([, items]) => items.length > 0)
    .map(([kind, items]) => ({ kind, heading: HEADING[kind](), items }));
}

/** What an approval covers: the whole grant when new, only the widening on an update. */
export function describeApproval(
  inspection: PluginInspection,
): DescribedSection[] {
  const grant = inspection.replaces ? inspection.widened : inspection.grant;
  return describeGrant(
    grant,
    inspection.unrestricted,
    inspection.programPaths,
  );
}

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
    lines.push(
      section.heading,
      ...section.items.map(({ label, detail }) =>
        detail ? `• ${label} — ${detail}` : `• ${label}`,
      ),
    );
  }

  if (lines.length <= MAX_DIALOG_LINES)
    return { title, message: lines.join('\n'), complete: true };
  const kept = lines.slice(0, MAX_DIALOG_LINES - 2);
  kept.push('', m['settings.plugins.approval.more']());
  return { title, message: kept.join('\n'), complete: false };
}
