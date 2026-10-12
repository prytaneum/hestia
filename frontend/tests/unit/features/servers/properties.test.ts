import { describe, expect, it } from 'vitest';
import {
  hardcoreLock,
  propertyEdits,
  serverProperties,
} from '@/features/servers/lib';

const entry = (key: string, value: string) => ({ key, value });

describe('reading server.properties out of the config list', () => {
  it('leaves out the settings kept on the record', () => {
    const properties = serverProperties([
      entry('memory', '4G'),
      entry('jvm-args', ''),
      entry('backup-interval', ''),
      entry('backup-retention', ''),
      entry('motd', 'hello'),
    ]);
    expect(properties.map((p) => p.key)).toEqual(['motd']);
  });

  it('reads a control off the stored value', () => {
    const controls = Object.fromEntries(
      serverProperties([
        entry('pvp', 'true'),
        entry('max-players', '20'),
        entry('motd', 'A Minecraft Server'),
        entry('difficulty', 'easy'),
      ]).map((p) => [p.key, p.control.type]),
    );
    expect(controls).toEqual({
      pvp: 'toggle',
      'max-players': 'number',
      motd: 'text',
      difficulty: 'choice',
    });
  });

  it('keeps a legacy numeric gamemode a number', () => {
    const [gamemode] = serverProperties([entry('gamemode', '0')]);
    expect(gamemode.control.type).toBe('number');
  });

  it('marks the keys the daemon owns', () => {
    const managed = serverProperties([
      entry('server-port', '25565'),
      entry('rcon.password', 'secret'),
      entry('motd', ''),
    ]).filter((p) => p.managed);
    expect(managed.map((p) => p.key)).toEqual(['rcon.password', 'server-port']);
  });
});

describe('the hardcore interlock', () => {
  it('pins difficulty and gamemode while hardcore is on', () => {
    expect(hardcoreLock('difficulty', { hardcore: 'true' })).toBe('hard');
    expect(hardcoreLock('gamemode', { hardcore: 'true' })).toBe('survival');
    expect(hardcoreLock('difficulty', { hardcore: 'false' })).toBeUndefined();
    expect(hardcoreLock('motd', { hardcore: 'true' })).toBeUndefined();
  });
});

describe('ordering the edits to send', () => {
  const properties = serverProperties([
    entry('hardcore', 'false'),
    entry('difficulty', 'easy'),
    entry('motd', 'old'),
  ]);

  it('sends only what differs from the stored value', () => {
    expect(propertyEdits(properties, { motd: 'old', difficulty: 'hard' })).toEqual(
      [entry('difficulty', 'hard')],
    );
  });

  it('writes the pinned keys before turning hardcore on', () => {
    const keys = propertyEdits(properties, {
      hardcore: 'true',
      difficulty: 'hard',
    }).map((e) => e.key);
    expect(keys).toEqual(['difficulty', 'hardcore']);
  });

  it('writes the pinned keys after turning hardcore off', () => {
    const on = serverProperties([
      entry('hardcore', 'true'),
      entry('difficulty', 'hard'),
    ]);
    const keys = propertyEdits(on, {
      difficulty: 'easy',
      hardcore: 'false',
    }).map((e) => e.key);
    expect(keys).toEqual(['hardcore', 'difficulty']);
  });
});
