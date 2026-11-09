import { expect, it } from 'vitest';
import { appendDevLog } from '@/features/settings/lib/dev-log';

it('keeps only the newest lines once the tail is full', () => {
  let lines = [{ level: 'info', text: 'a' }];
  lines = appendDevLog(lines, { level: 'info', text: 'b' }, 2);
  lines = appendDevLog(lines, { level: 'warn', text: 'c' }, 2);
  expect(lines.map((line) => line.text)).toEqual(['b', 'c']);
});
