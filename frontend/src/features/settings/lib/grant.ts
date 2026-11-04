import type { Grant, Permission } from '@/api/types';

type Tier = 'unrestricted' | 'sandboxed';

/**
 * Mirrors `Permission::tier` in `proto`. Keyed by the generated union, so a
 * permission the daemon gains fails typecheck here until it is placed.
 */
const TIER: Record<Permission, Tier> = {
  'entry:read': 'sandboxed',
  'entry:write': 'unrestricted',
  'process:read': 'sandboxed',
  'process:control': 'unrestricted',
  'process:spawn': 'unrestricted',
  'content:read': 'sandboxed',
  'content:write': 'unrestricted',
  'account:read': 'sandboxed',
  'config:read': 'sandboxed',
  'config:write': 'unrestricted',
  'java:read': 'sandboxed',
  'java:manage': 'sandboxed',
  'network:connect': 'sandboxed',
  'ui:css': 'sandboxed',
  'launch:patch': 'sandboxed',
};

export type GrantSectionKind =
  | 'full_access'
  | 'permissions'
  | 'programs'
  | 'network'
  | 'launch_properties'
  | 'hooks'
  | 'events'
  | 'secret_settings'
  | 'themes';

export interface GrantSection {
  kind: GrantSectionKind;
  /** Permission names for the two permission sections, display text otherwise. */
  items: string[];
}

export const hasFullAccess = (grant: Grant): boolean =>
  grant.permissions.some((permission) => TIER[permission] === 'unrestricted');

/**
 * What a grant asks for, as non-empty sections in the order a person should
 * weigh them: full access first, then the sandboxed permissions, then the
 * narrower lists.
 */
export function grantSections(grant: Grant): GrantSection[] {
  const permissionsOf = (tier: Tier) =>
    grant.permissions.filter((permission) => TIER[permission] === tier);
  const sections: GrantSection[] = [
    { kind: 'full_access', items: permissionsOf('unrestricted') },
    { kind: 'permissions', items: permissionsOf('sandboxed') },
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
