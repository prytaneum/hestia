import { describe, expect, it } from 'vitest';
import type { Setting } from '@/api';
import {
  groupSettings,
  settingProblem,
  topicMatches,
} from '@/features/plugins/lib';

const setting = (over: Partial<Setting>): Setting => ({
  key: 'k',
  kind: 'string',
  default: null,
  label: '',
  help: '',
  options: [],
  placeholder: '',
  group: '',
  ...over,
});

describe('topicMatches', () => {
  it('matches an exact topic or everything beneath a prefix glob', () => {
    expect(topicMatches('job', 'job')).toBe(true);
    expect(topicMatches('job', 'job.x')).toBe(false);
    expect(topicMatches('server.*', 'server.ready')).toBe(true);
    expect(topicMatches('server.*', 'server')).toBe(false);
    expect(topicMatches('server.*', 'servers.x')).toBe(false);
    expect(topicMatches('server*', 'server.ready')).toBe(false);
  });
});

describe('settingProblem', () => {
  it('holds a number to its bounds', () => {
    const port = setting({ kind: 'number', min: 1, max: 65535 });
    expect(settingProblem(port, '8080')).toBeNull();
    expect(settingProblem(port, '0')).toEqual({ kind: 'min', min: 1 });
    expect(settingProblem(port, '70000')).toEqual({ kind: 'max', max: 65535 });
  });

  it('matches a pattern against the whole text', () => {
    const name = setting({ pattern: '[a-z]{2,4}' });
    expect(settingProblem(name, 'abc')).toBeNull();
    expect(settingProblem(name, 'abcde')).toEqual({ kind: 'pattern' });
    expect(settingProblem(name, 'xabc!')).toEqual({ kind: 'pattern' });
  });

  it('says nothing about an empty value or an unusable pattern', () => {
    expect(settingProblem(setting({ pattern: 'a' }), '')).toBeNull();
    expect(settingProblem(setting({ pattern: '(' }), 'x')).toBeNull();
  });
});

describe('groupSettings', () => {
  it('lists ungrouped settings first and groups in first-seen order', () => {
    const grouped = groupSettings([
      setting({ key: 'a', group: 'Network' }),
      setting({ key: 'b' }),
      setting({ key: 'c', group: 'Network' }),
      setting({ key: 'd', group: 'Display' }),
    ]);
    expect(grouped.map(([group, rows]) => [group, rows.map((s) => s.key)])).toEqual(
      [
        ['', ['b']],
        ['Network', ['a', 'c']],
        ['Display', ['d']],
      ],
    );
  });
});
