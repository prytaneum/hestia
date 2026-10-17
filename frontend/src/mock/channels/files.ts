/**
 * `server.files.*` over an in-memory tree per server: a path maps to its text,
 * or to `null` for a directory. Archives are not modelled — compressing writes
 * a placeholder and extracting makes the directory.
 */
import type { FileEntry } from '@/api/types';

import { ago, type ChannelHandlers, fail, ok, str, strings } from '../support';

type Tree = Map<string, string | null>;

const BINARY = '\u0000';

const seed = (): Tree =>
  new Map<string, string | null>([
    ['world', null],
    ['world/level.dat', BINARY],
    ['world/region', null],
    ['world/region/r.0.0.mca', BINARY],
    ['plugins', null],
    ['plugins/Essentials', null],
    [
      'plugins/Essentials/config.yml',
      'ops-name-color: "4"\nnickname-prefix: "~"\n',
    ],
    ['logs', null],
    ['logs/latest.log', '[12:00:00] [Server thread/INFO]: Done (3.412s)!\n'],
    ['server.properties', 'motd=A Mock Server\nmax-players=20\npvp=true\n'],
    ['ops.json', '[]\n'],
    ['eula.txt', 'eula=true\n'],
    ['server.jar', BINARY],
  ]);

const trees = new Map<string, Tree>();

function treeOf(server: string): Tree {
  let tree = trees.get(server);
  if (!tree) {
    tree = seed();
    trees.set(server, tree);
  }
  return tree;
}

const parentOf = (path: string): string =>
  path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';

const nameOf = (path: string): string => path.slice(path.lastIndexOf('/') + 1);

const under = (path: string, root: string): boolean =>
  path === root || path.startsWith(`${root}/`);

function refuse(path: string, reason: string): never {
  fail('bad_request', `'${path}' was refused: ${reason}`);
}

function existing(tree: Tree, path: string): string {
  if (!tree.has(path)) fail('not_found', `no file or directory at '${path}'`);
  return path;
}

function free(tree: Tree, path: string): string {
  if (path === '' || tree.has(path)) {
    refuse(path, 'something with that name already exists');
  }
  return path;
}

/** Create every missing directory above `path`. */
function ensureParents(tree: Tree, path: string): void {
  for (let dir = parentOf(path); dir !== ''; dir = parentOf(dir)) {
    if (!tree.has(dir)) tree.set(dir, null);
  }
}

export const channels: ChannelHandlers = {
  'server.files.list': (p) => {
    const tree = treeOf(str(p, 'server'));
    const dir = str(p, 'path');
    if (dir !== '' && tree.get(existing(tree, dir)) !== null) {
      refuse(dir, 'it is not a directory');
    }
    const entries: FileEntry[] = [...tree.entries()]
      .filter(([path]) => parentOf(path) === dir)
      .map(([path, content]) => ({
        name: nameOf(path),
        path,
        dir: content === null,
        size: content?.length ?? 0,
        modifiedUnix: ago(3600),
      }))
      .sort(
        (a, b) => Number(b.dir) - Number(a.dir) || a.name.localeCompare(b.name),
      );
    return { entries };
  },

  'server.files.read': (p) => {
    const tree = treeOf(str(p, 'server'));
    const path = existing(tree, str(p, 'path'));
    const content = tree.get(path);
    if (content == null) refuse(path, 'it is a directory');
    if (content === BINARY) refuse(path, 'it is not a text file');
    return { content };
  },

  'server.files.write': (p) => {
    const tree = treeOf(str(p, 'server'));
    const path = str(p, 'path');
    if (tree.get(path) === null) refuse(path, 'it is a directory');
    ensureParents(tree, path);
    tree.set(path, str(p, 'content'));
    return ok();
  },

  'server.files.mkdir': (p) => {
    const tree = treeOf(str(p, 'server'));
    const path = free(tree, str(p, 'path'));
    ensureParents(tree, path);
    tree.set(path, null);
    return ok();
  },

  'server.files.move': (p) => {
    const tree = treeOf(str(p, 'server'));
    const from = existing(tree, str(p, 'from'));
    const to = free(tree, str(p, 'to'));
    ensureParents(tree, to);
    for (const [path, content] of [...tree.entries()]) {
      if (!under(path, from)) continue;
      tree.delete(path);
      tree.set(to + path.slice(from.length), content);
    }
    return ok();
  },

  'server.files.copy': (p) => {
    const tree = treeOf(str(p, 'server'));
    const from = existing(tree, str(p, 'from'));
    const to = free(tree, str(p, 'to'));
    ensureParents(tree, to);
    for (const [path, content] of [...tree.entries()]) {
      if (under(path, from)) tree.set(to + path.slice(from.length), content);
    }
    return ok();
  },

  'server.files.remove': (p) => {
    const tree = treeOf(str(p, 'server'));
    const targets = strings(p, 'paths').map((path) => existing(tree, path));
    for (const path of [...tree.keys()]) {
      if (targets.some((target) => under(path, target))) tree.delete(path);
    }
    return ok();
  },

  'server.files.import': (p) => {
    const tree = treeOf(str(p, 'server'));
    const dir = str(p, 'path');
    for (const source of strings(p, 'sources')) {
      const name = source.split(/[\\/]/).pop() ?? source;
      tree.set(dir === '' ? name : `${dir}/${name}`, BINARY);
    }
    return ok();
  },

  'server.files.compress': (p) => {
    const tree = treeOf(str(p, 'server'));
    for (const path of strings(p, 'paths')) existing(tree, path);
    tree.set(free(tree, str(p, 'to')), BINARY);
    return ok();
  },

  'server.files.extract': (p) => {
    const tree = treeOf(str(p, 'server'));
    existing(tree, str(p, 'from'));
    const to = str(p, 'to');
    if (to !== '' && !tree.has(to)) {
      ensureParents(tree, to);
      tree.set(to, null);
    }
    return ok();
  },
};
