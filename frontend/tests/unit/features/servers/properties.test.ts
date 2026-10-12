import { describe, expect, it } from 'vitest';
import {
  hardcoreLock,
  propertyWrites,
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

describe('the writes one change takes', () => {
  const properties = serverProperties([
    entry('hardcore', 'false'),
    entry('difficulty', 'easy'),
    entry('gamemode', 'survival'),
    entry('motd', 'old'),
  ]);

  it('is the change itself for an ordinary key', () => {
    expect(propertyWrites(properties, 'motd', 'new')).toEqual([
      entry('motd', 'new'),
    ]);
  });

  it('moves what hardcore pins before turning it on', () => {
    expect(propertyWrites(properties, 'hardcore', 'true')).toEqual([
      entry('difficulty', 'hard'),
      entry('hardcore', 'true'),
    ]);
  });

  it('turns hardcore off on its own', () => {
    expect(propertyWrites(properties, 'hardcore', 'false')).toEqual([
      entry('hardcore', 'false'),
    ]);
  });

  it('leaves a legacy numeric value for the daemon to judge', () => {
    const legacy = serverProperties([
      entry('hardcore', 'false'),
      entry('difficulty', '1'),
    ]);
    expect(propertyWrites(legacy, 'hardcore', 'true')).toEqual([
      entry('hardcore', 'true'),
    ]);
  });
});
