import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, it } from 'vitest';
import { search } from '@/features/settings/lib/search';

const SETTINGS = join(import.meta.dirname, '..', '..', '..', '..', 'src', 'features', 'settings');

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sources(path);
    return path.endsWith('.tsx') ? [readFileSync(path, 'utf8')] : [];
  });
}

it('indexes every <Setting id> so an active search cannot hide it', () => {
  const rendered = sources(SETTINGS).flatMap((src) =>
    [...src.matchAll(/<Setting\s+id="([^"]+)"/g)].map((hit) => hit[1]),
  );
  const indexed = search('').ids;
  expect(rendered.length).toBeGreaterThan(0);
  expect(rendered.filter((id) => !indexed.has(id))).toEqual([]);
});
