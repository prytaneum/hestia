import { describe, expect, it } from 'vitest';
import { type ConsoleEntry, interleaveConsole } from '@/queries/console';

const echo = (text: string, after: string | null): ConsoleEntry => ({
  kind: 'echo',
  text,
  after,
});

describe('interleaving console entries with the log', () => {
  it('places a command after the line it followed', () => {
    const list = echo('» list', 'a');
    expect(interleaveConsole(['a', 'b'], [list])).toEqual(['a', list, 'b']);
  });

  it('keeps entries sharing an anchor in the order they were sent', () => {
    const first = echo('» one', 'a');
    const second = echo('» two', 'a');
    expect(interleaveConsole(['a', 'b'], [first, second])).toEqual([
      'a',
      first,
      second,
      'b',
    ]);
  });

  it('leads with a command sent into an empty log', () => {
    const early = echo('» early', null);
    expect(interleaveConsole(['a'], [early])).toEqual([early, 'a']);
  });

  it('drops an entry whose anchor has left the buffer', () => {
    expect(interleaveConsole(['b', 'c'], [echo('» old', 'a')])).toEqual([
      'b',
      'c',
    ]);
  });

  it('never orders a later entry before an earlier one', () => {
    const first = echo('» one', 'b');
    const second = echo('» two', 'gone');
    expect(interleaveConsole(['a', 'b', 'c'], [first, second])).toEqual([
      'a',
      'b',
      first,
      second,
      'c',
    ]);
  });
});
