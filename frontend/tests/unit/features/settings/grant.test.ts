import { expect, it } from 'vitest';
import type { Grant } from '@/api/types';
import { grantSections } from '@/features/settings/lib/grant';

const grant = (overrides: Partial<Grant>): Grant => ({
  permissions: [],
  network: [],
  programs: [],
  launchProperties: [],
  hooks: [],
  events: [],
  secretSettings: [],
  uiCss: [],
  ...overrides,
});

it('lists full access before sandboxed permissions and drops empty sections', () => {
  const sections = grantSections(
    grant({
      permissions: ['entry:read', 'entry:write', 'network:connect'],
      network: ['example.com'],
    }),
  );

  expect(sections).toEqual([
    { kind: 'full_access', items: ['entry:write'] },
    { kind: 'permissions', items: ['entry:read', 'network:connect'] },
    { kind: 'network', items: ['example.com'] },
  ]);
});

it('describes a program by id and command line', () => {
  const [programs] = grantSections(
    grant({
      programs: [
        { id: 'git', command: 'git', args: ['status'], env: [], sha256: {} },
      ],
    }),
  );

  expect(programs).toEqual({ kind: 'programs', items: ['git: git status'] });
});
