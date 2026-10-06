import { describe, expect, it } from 'vitest';
import {
  intervalDraft,
  intervalIssue,
  intervalValue,
} from '@/features/servers/lib';

describe('intervalDraft', () => {
  it('opens an unset interval as off', () => {
    expect(intervalDraft('').mode).toBe('off');
  });

  it('opens a preset as itself, carrying its amount into custom', () => {
    expect(intervalDraft('6h')).toEqual({ mode: '6h', count: '6', unit: 'h' });
  });

  it('opens anything else as custom', () => {
    expect(intervalDraft('45m')).toEqual({
      mode: 'custom',
      count: '45',
      unit: 'm',
    });
  });
});

describe('intervalIssue', () => {
  const custom = (count: string, unit: 'm' | 'h' | 'd') =>
    intervalIssue({ mode: 'custom', count, unit });

  it('ignores the amount outside custom', () => {
    expect(intervalIssue({ mode: 'off', count: '', unit: 'm' })).toBeNull();
  });

  it('wants a positive whole number', () => {
    expect(custom('', 'h')).toBe('whole_number');
    expect(custom('0', 'h')).toBe('whole_number');
    expect(custom('1.5', 'h')).toBe('whole_number');
  });

  it('holds the five-minute floor', () => {
    expect(custom('4', 'm')).toBe('too_short');
    expect(custom('5', 'm')).toBeNull();
    expect(custom('1', 'h')).toBeNull();
  });
});

describe('intervalValue', () => {
  it('writes empty for off, the preset, or the custom amount', () => {
    expect(intervalValue({ mode: 'off', count: '1', unit: 'h' })).toBe('');
    expect(intervalValue({ mode: '12h', count: '12', unit: 'h' })).toBe('12h');
    expect(intervalValue({ mode: 'custom', count: '03', unit: 'd' })).toBe(
      '3d',
    );
  });
});
