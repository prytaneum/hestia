import { describe, expect, it } from 'vitest';

import { bytes, compact, memGb, RateMeter, uptime } from '@/lib/format';

describe('memGb', () => {
  it('reads the unit rather than assuming gigabytes', () => {
    expect(memGb('8G')).toBe(8);
    expect(memGb('8192M')).toBe(8);
    expect(memGb('8388608K')).toBe(8);
    expect(memGb('4g')).toBe(4);
  });

  it('rounds to the gigabyte the surfaces offer, never below one', () => {
    expect(memGb('6656M')).toBe(7);
    expect(memGb('512M')).toBe(1);
  });

  it('falls back to 4 for anything unparseable', () => {
    expect(memGb('')).toBe(4);
    expect(memGb('8')).toBe(4);
    expect(memGb('4GB')).toBe(4);
  });
});

describe('bytes', () => {
  it('stays in bytes below a kilobyte', () => {
    expect(bytes(0)).toBe('0 B');
    expect(bytes(1023)).toBe('1023 B');
  });

  it('climbs one unit at a time', () => {
    expect(bytes(1024)).toBe('1.0 KB');
    expect(bytes(1024 ** 2)).toBe('1.0 MB');
    expect(bytes(1024 ** 3)).toBe('1.0 GB');
  });

  it('stops at gigabytes rather than inventing a larger unit', () => {
    expect(bytes(1024 ** 4)).toBe('1024 GB');
  });

  it('drops the decimal once the value reaches ten', () => {
    expect(bytes(9.5 * 1024)).toBe('9.5 KB');
    expect(bytes(10 * 1024)).toBe('10 KB');
  });
});

describe('uptime', () => {
  it('shows the two largest units that matter', () => {
    expect(uptime(45)).toBe('45s');
    expect(uptime(125)).toBe('2m 5s');
    expect(uptime(3725)).toBe('1h 2m');
    expect(uptime(90_000)).toBe('1d 1h');
  });

  it('never reports a negative or fractional duration', () => {
    expect(uptime(-10)).toBe('0s');
    expect(uptime(1.9)).toBe('1s');
  });
});

describe('compact', () => {
  it('abbreviates thousands and millions', () => {
    expect(compact(999)).toBe('999');
    expect(compact(1_500)).toBe('1.5k');
    expect(compact(2_400_000)).toBe('2.4M');
  });
});

describe('RateMeter', () => {
  it('reports nothing until a full window has elapsed', () => {
    const meter = new RateMeter();
    expect(meter.observe(0, 0)).toBe(0);
    expect(meter.observe(1000, 100)).toBe(0);
  });

  it('averages over the window rather than over each burst', () => {
    const meter = new RateMeter();
    meter.observe(0, 0);
    expect(meter.observe(1000, 1000)).toBe(1000);
  });

  it('treats a counter going backwards as a new stream', () => {
    const meter = new RateMeter();
    meter.observe(0, 0);
    meter.observe(1000, 1000);
    expect(meter.observe(10, 2000)).toBe(0);
  });
});
