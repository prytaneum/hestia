import { describe, expect, it } from 'vitest';
import {
  archiveStem,
  baseName,
  editorLanguage,
  freeName,
  isArchive,
  joinPath,
  nameIssue,
  parentPath,
  pathCrumbs,
} from '@/features/servers/lib';

describe('wire paths', () => {
  it('joins under the root without a leading slash', () => {
    expect(joinPath('', 'world')).toBe('world');
    expect(joinPath('plugins', 'a.jar')).toBe('plugins/a.jar');
  });

  it('splits a path into its parent and its name', () => {
    expect(parentPath('plugins/Essentials/config.yml')).toBe(
      'plugins/Essentials',
    );
    expect(parentPath('server.properties')).toBe('');
    expect(baseName('plugins/Essentials/config.yml')).toBe('config.yml');
    expect(baseName('eula.txt')).toBe('eula.txt');
  });

  it('lists every directory down to the path as crumbs', () => {
    expect(pathCrumbs('')).toEqual([]);
    expect(pathCrumbs('world/region')).toEqual([
      { name: 'world', path: 'world' },
      { name: 'region', path: 'world/region' },
    ]);
  });
});

describe('archives', () => {
  it('knows the formats the daemon unpacks', () => {
    expect(isArchive('world.zip')).toBe(true);
    expect(isArchive('World.TAR.GZ')).toBe(true);
    expect(isArchive('backup.tgz')).toBe(true);
    expect(isArchive('server.jar')).toBe(false);
  });

  it('names the folder an archive unpacks into after its stem', () => {
    expect(archiveStem('world.zip')).toBe('world');
    expect(archiveStem('world.tar.gz')).toBe('world');
    expect(archiveStem('notes.txt')).toBe('notes.txt');
  });
});

describe('picking a name', () => {
  it('keeps a free name and numbers a taken one before its extension', () => {
    expect(freeName('ops.json', ['eula.txt'])).toBe('ops.json');
    expect(freeName('ops.json', ['ops.json'])).toBe('ops (2).json');
    expect(freeName('ops.json', ['ops.json', 'ops (2).json'])).toBe(
      'ops (3).json',
    );
    expect(freeName('world', ['world'])).toBe('world (2)');
  });

  it('says why a name cannot be used', () => {
    expect(nameIssue('  ', [])).toBe('empty');
    expect(nameIssue('..', [])).toBe('empty');
    expect(nameIssue('a/b', [])).toBe('separator');
    expect(nameIssue('a\\b', [])).toBe('separator');
    expect(nameIssue('world', ['world'])).toBe('taken');
    expect(nameIssue('new.txt', ['world'])).toBeNull();
  });
});

describe('the editor language', () => {
  it('follows the file extension and falls back to plain text', () => {
    expect(editorLanguage('ops.json')).toBe('json');
    expect(editorLanguage('config.YML')).toBe('yaml');
    expect(editorLanguage('server.properties')).toBe('properties');
    expect(editorLanguage('paper.toml')).toBe('toml');
    expect(editorLanguage('latest.log')).toBe('plain');
    expect(editorLanguage('LICENSE')).toBe('plain');
  });
});
