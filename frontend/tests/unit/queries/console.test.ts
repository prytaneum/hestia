import { describe, expect, it } from 'vitest';
import type { ProcessLogLine } from '@/api';
import { type ConsoleEntry, interleaveConsole } from '@/queries/console';

const line = (text: string, offset: number): ProcessLogLine => ({
  stream: 'stdout',
  line: text,
  offset,
});

const echo = (text: string, after: ProcessLogLine | null): ConsoleEntry => ({
  kind: 'echo',
  text,
  after,
});

const a = line('a', 0);
const b = line('b', 2);
const c = line('c', 4);

describe('interleaving console entries with the log', () => {
  it('places a command after the line it followed', () => {
    const list = echo('» list', a);
    expect(interleaveConsole([a, b], [list])).toEqual([a, list, b]);
  });

  it('keeps entries sharing an anchor in the order they were sent', () => {
    const first = echo('» one', a);
    const second = echo('» two', a);
    expect(interleaveConsole([a, b], [first, second])).toEqual([
      a,
      first,
      second,
      b,
    ]);
  });

  it('leads with a command sent into an empty log', () => {
    const early = echo('» early', null);
    expect(interleaveConsole([a], [early])).toEqual([early, a]);
  });

  it('drops an entry whose anchor has left the buffer', () => {
    expect(interleaveConsole([b, c], [echo('» old', a)])).toEqual([b, c]);
  });

  it('never orders a later entry before an earlier one', () => {
    const first = echo('» one', b);
    const second = echo('» two', line('gone', 99));
    expect(interleaveConsole([a, b, c], [first, second])).toEqual([
      a,
      b,
      first,
      second,
      c,
    ]);
  });

  it('tells a repeated line from the one it followed', () => {
    const again = line('a', 6);
    const list = echo('» list', a);
    expect(interleaveConsole([a, b, again], [list])).toEqual([
      a,
      list,
      b,
      again,
    ]);
  });

  it('drops an entry anchored in a log file since replaced', () => {
    const replaced = line('fresh', 0);
    expect(interleaveConsole([replaced], [echo('» old', a)])).toEqual([
      replaced,
    ]);
  });
});
