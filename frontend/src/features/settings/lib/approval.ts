import type { Permission, PluginInspection } from '@/api/types';
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

interface GrantSection {
  kind: GrantSectionKind;
  items: string[];
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

export interface DescribedSection {
  heading: string;
  lines: string[];
}

/**
 * What an approval covers: the whole grant when new, only the widening on an
 * update. Which permissions are full access is the daemon's call
 * (`inspection.unrestricted`), listed first.
 */
function sectionsToApprove(inspection: PluginInspection): GrantSection[] {
  const grant = inspection.replaces ? inspection.widened : inspection.grant;
  const full = new Set<Permission>(inspection.unrestricted);
  const sections: GrantSection[] = [
    { kind: 'full_access', items: inspection.unrestricted },
    {
      kind: 'permissions',
      items: grant.permissions.filter((permission) => !full.has(permission)),
    },
    {
      kind: 'programs',
      items: grant.programs.map((program) =>
        [program.id, [program.command, ...program.args].join(' ')].join(': '),
      ),
    },
    { kind: 'network', items: grant.network },
    { kind: 'launch_properties', items: grant.launchProperties },
    { kind: 'hooks', items: grant.hooks },
    { kind: 'events', items: grant.events },
    { kind: 'secret_settings', items: grant.secretSettings },
    { kind: 'themes', items: grant.uiCss },
  ];
  return sections.filter((section) => section.items.length > 0);
}

export function describeApproval(
  inspection: PluginInspection,
): DescribedSection[] {
  return sectionsToApprove(inspection).map(({ kind, items }) => ({
    heading: HEADING[kind](),
    lines:
      kind === 'full_access' || kind === 'permissions'
        ? items.map(
            (item) => `${item} — ${PERMISSION_TEXT[item as Permission]()}`,
          )
        : items,
  }));
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
    lines.push(section.heading, ...section.lines.map((line) => `• ${line}`));
  }

  if (lines.length <= MAX_DIALOG_LINES)
    return { title, message: lines.join('\n'), complete: true };
  const kept = lines.slice(0, MAX_DIALOG_LINES - 2);
  kept.push('', m['settings.plugins.approval.more']());
  return { title, message: kept.join('\n'), complete: false };
}
