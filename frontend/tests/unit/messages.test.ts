/**
 * Guards the message catalogue against the two ways it drifts: a locale falling
 * behind the base one, and keys outliving their call sites.
 *
 * Keys reached dynamically (`m[`error.kind.${kind}`]`) have no literal call
 * site, so every such table is declared in `DYNAMIC_PREFIXES` below. Adding a
 * dynamic lookup without listing it here fails the dead-key test — deliberately,
 * since an undeclared table is exactly what makes dead keys unprovable.
 */
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const ROOT = join(import.meta.dirname, '..', '..');
const MESSAGES = join(ROOT, 'messages');
const BASE_LOCALE = 'en';

const DYNAMIC_PREFIXES = [
  'app.crash.',
  'app.jobs.kind.',
  'error.kind.',
  'error.code.',
  'error.token.',
  'warning.kind.',
  'warning.hint.',
  'warning.token.',
  'domain.entry_type.',
  'domain.gamemode.',
  'domain.difficulty.',
  'domain.flavor.',
  'domain.export_format.',
  'domain.import_format.',
];

function flatten(value: unknown, prefix = ''): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, child] of Object.entries(value as object)) {
    if (key === '$schema') continue;
    const path = `${prefix}${key}`;
    if (typeof child === 'object' && child !== null)
      Object.assign(out, flatten(child, `${path}.`));
    else out[path] = child as string;
  }
  return out;
}

function catalogue(locale: string): Record<string, string> {
  const dir = join(MESSAGES, locale);
  const out: Record<string, string> = {};
  for (const file of readdirSync(dir).filter((f) => f.endsWith('.json')))
    Object.assign(out, flatten(JSON.parse(readFileSync(join(dir, file), 'utf8'))));
  return out;
}

function sources(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'paraglide') continue;
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...sources(path));
    else if (/\.tsx?$/.test(entry.name)) out.push(readFileSync(path, 'utf8'));
  }
  return out;
}

function placeholders(message: string): string[] {
  return [...message.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
}

const locales = readdirSync(MESSAGES, { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name);

const base = catalogue(BASE_LOCALE);
const referenced = new Set(
  sources(join(ROOT, 'src')).flatMap((text) => [
    ...[...text.matchAll(/m(?:sg|essages)?\[\s*['"]([^'"]+)['"]\s*\]/g)].map(
      (match) => match[1],
    ),
  ]),
);

const GENERATED = join(ROOT, 'src', 'api', 'types', 'generated');
const ERROR_TOKEN_ENUMS = [
  'ArchiveFault',
  'ArchiveFormat',
  'Field',
  'IoOp',
  'Reason',
  'Service',
  'Task',
  'Unsupported',
  'ValueKind',
];

function generated(type: string): string {
  return readFileSync(join(GENERATED, `${type}.ts`), 'utf8');
}

function unionMembers(type: string): string[] {
  const declaration = generated(type).split(`export type ${type} =`)[1] ?? '';
  return [...declaration.matchAll(/"(\w+)"/g)].map((match) => match[1]);
}

describe('daemon error vocabulary', () => {
  it('has a message for every error kind', () => {
    const kinds = [...generated('ErrorInfo').matchAll(/"kind": "(\w+)"/g)].map(
      (match) => match[1],
    );
    expect(kinds.length).toBeGreaterThan(0);
    expect(kinds.filter((kind) => !(`error.kind.${kind}` in base))).toEqual([]);
  });

  it.each(ERROR_TOKEN_ENUMS)('has a label for every %s value', (type) => {
    const members = unionMembers(type);
    expect(members.length).toBeGreaterThan(0);
    expect(members.filter((value) => !(`error.token.${value}` in base))).toEqual(
      [],
    );
  });
});

describe('job titles', () => {
  it('has a title for every job channel', () => {
    const channels = [
      ...readFileSync(join(GENERATED, '..', 'channels.ts'), 'utf8')
        .split('export interface Jobs {')[1]
        .split('\n}')[0]
        .matchAll(/^\s+"([\w.]+)":/gm),
    ].map((match) => match[1]);
    expect(channels.length).toBeGreaterThan(0);
    expect(
      channels.filter((channel) => !(`app.jobs.kind.${channel}` in base)),
    ).toEqual([]);
  });
});

describe('message catalogue', () => {
  it('lists more than one locale', () => {
    expect(locales).toContain(BASE_LOCALE);
    expect(locales.length).toBeGreaterThan(1);
  });

  it.each(locales.filter((l) => l !== BASE_LOCALE))(
    '%s covers the base locale exactly',
    (locale) => {
      const keys = Object.keys(catalogue(locale));
      expect(Object.keys(base).filter((k) => !keys.includes(k))).toEqual([]);
      expect(keys.filter((k) => !(k in base))).toEqual([]);
    },
  );

  it.each(locales.filter((l) => l !== BASE_LOCALE))(
    '%s interpolates the same placeholders',
    (locale) => {
      const translated = catalogue(locale);
      const mismatched = Object.keys(base).filter(
        (key) =>
          key in translated &&
          placeholders(base[key]).join() !== placeholders(translated[key]).join(),
      );
      expect(mismatched).toEqual([]);
    },
  );

  it('defines every key the source references', () => {
    expect([...referenced].filter((key) => !(key in base)).sort()).toEqual([]);
  });

  it('has no message without a call site', () => {
    const dead = Object.keys(base).filter(
      (key) =>
        !referenced.has(key) &&
        !DYNAMIC_PREFIXES.some((prefix) => key.startsWith(prefix)),
    );
    expect(dead.sort()).toEqual([]);
  });
});
