import { describe, expect, it } from 'vitest';

import {
  MAX_BRIDGE_BYTES,
  parseBridgeMessage,
} from '@/features/plugins/view/bridge';

describe('parseBridgeMessage', () => {
  it('accepts a run request', () => {
    expect(
      parseBridgeMessage({
        id: '1',
        type: 'run',
        command: 'sync',
        args: { all: true },
      }),
    ).toEqual({ id: '1', type: 'run', command: 'sync', args: { all: true } });
  });

  it('defaults a run request without arguments', () => {
    expect(
      parseBridgeMessage({ id: '1', type: 'run', command: 'sync' }),
    ).toEqual({ id: '1', type: 'run', command: 'sync', args: {} });
  });

  it('accepts an action request', () => {
    expect(
      parseBridgeMessage({
        id: '2',
        type: 'action',
        action: 'save',
        payload: { x: 1 },
      }),
    ).toEqual({ id: '2', type: 'action', action: 'save', payload: { x: 1 } });
  });

  it('drops anything that is not a plain object', () => {
    for (const data of [null, undefined, 'run', 3, ['run'], new Map()]) {
      expect(parseBridgeMessage(data)).toBeNull();
    }
  });

  it('drops an unknown type or a missing id', () => {
    expect(
      parseBridgeMessage({ id: '1', type: 'eval', command: 'x' }),
    ).toBeNull();
    expect(parseBridgeMessage({ type: 'run', command: 'x' })).toBeNull();
    expect(
      parseBridgeMessage({ id: 1, type: 'run', command: 'x' }),
    ).toBeNull();
    expect(
      parseBridgeMessage({ id: '', type: 'run', command: 'x' }),
    ).toBeNull();
  });

  it('drops malformed fields', () => {
    expect(parseBridgeMessage({ id: '1', type: 'run', command: 5 })).toBeNull();
    expect(
      parseBridgeMessage({ id: '1', type: 'run', command: 'x', args: [1] }),
    ).toBeNull();
    expect(parseBridgeMessage({ id: '1', type: 'action' })).toBeNull();
  });

  it('drops a message over the size cap', () => {
    const big = 'x'.repeat(MAX_BRIDGE_BYTES);
    expect(
      parseBridgeMessage({ id: '1', type: 'run', command: 'x', args: { big } }),
    ).toBeNull();
  });

  it('drops a cyclic message', () => {
    const args: Record<string, unknown> = {};
    args.self = args;
    expect(
      parseBridgeMessage({ id: '1', type: 'run', command: 'x', args }),
    ).toBeNull();
  });
});
