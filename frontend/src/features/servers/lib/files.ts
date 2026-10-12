/**
 * Wire paths in a server's file tree: relative to the game directory,
 * `/`-separated on every platform, the empty string for the root.
 */

export function joinPath(dir: string, name: string): string {
  return dir === '' ? name : `${dir}/${name}`;
}

export function parentPath(path: string): string {
  const at = path.lastIndexOf('/');
  return at < 0 ? '' : path.slice(0, at);
}

export function baseName(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1);
}

/** Each directory from the root down to `path`, for a breadcrumb trail. */
export function pathCrumbs(path: string): { name: string; path: string }[] {
  if (path === '') return [];
  const parts = path.split('/');
  return parts.map((name, i) => ({
    name,
    path: parts.slice(0, i + 1).join('/'),
  }));
}

const ARCHIVE_SUFFIXES = ['.zip', '.tar.gz', '.tgz'];

/** Whether the daemon can unpack `name` — the formats `extract` takes. */
export function isArchive(name: string): boolean {
  const lower = name.toLowerCase();
  return ARCHIVE_SUFFIXES.some((suffix) => lower.endsWith(suffix));
}

/** `name` without its archive suffix: where an archive unpacks by default. */
export function archiveStem(name: string): string {
  const lower = name.toLowerCase();
  const suffix = ARCHIVE_SUFFIXES.find((candidate) =>
    lower.endsWith(candidate),
  );
  return suffix ? name.slice(0, name.length - suffix.length) : name;
}

/**
 * A name a new sibling can take: `name` itself, or the first `name (n)` /
 * `stem (n).ext` that `taken` does not hold.
 */
export function freeName(name: string, taken: readonly string[]): string {
  if (!taken.includes(name)) return name;
  const dot = name.lastIndexOf('.');
  const [stem, extension] =
    dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ''];
  for (let n = 2; ; n++) {
    const candidate = `${stem} (${n})${extension}`;
    if (!taken.includes(candidate)) return candidate;
  }
}

/** Why `name` cannot name a file here, or null when it can. */
export function nameIssue(
  name: string,
  taken: readonly string[],
): 'empty' | 'separator' | 'taken' | null {
  const trimmed = name.trim();
  if (trimmed === '' || trimmed === '.' || trimmed === '..') return 'empty';
  if (/[\\/]/.test(trimmed)) return 'separator';
  return taken.includes(trimmed) ? 'taken' : null;
}

export type EditorLanguage = 'json' | 'yaml' | 'properties' | 'toml' | 'plain';

const LANGUAGES: Record<string, EditorLanguage> = {
  json: 'json',
  json5: 'json',
  mcmeta: 'json',
  yml: 'yaml',
  yaml: 'yaml',
  properties: 'properties',
  cfg: 'properties',
  conf: 'properties',
  ini: 'properties',
  toml: 'toml',
};

/** The syntax a file is highlighted as, by its extension. */
export function editorLanguage(name: string): EditorLanguage {
  const extension = name.slice(name.lastIndexOf('.') + 1).toLowerCase();
  return LANGUAGES[extension] ?? 'plain';
}
