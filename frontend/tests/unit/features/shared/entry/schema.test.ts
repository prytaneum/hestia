import { describe, expect, it } from 'vitest';
import type { KeyRule } from '@/api/types';
import { randomIconConfig } from '@/features/shared/entry/components/icon-editor/catalog';
import {
  detailsStepSchema,
  flavorStepSchema,
  versionStepSchema,
} from '@/features/shared/entry/lib';

const details = (over: Record<string, unknown> = {}) => ({
  name: 'smp',
  memory: 4,
  motd: '',
  seed: '',
  gamemode: 'survival',
  difficulty: 'normal',
  maxPlayers: '20',
  port: '',
  hardcore: false,
  onlineMode: true,
  icon: randomIconConfig(),
  eula: true,
  ...over,
});

describe('flavorStepSchema', () => {
  it('requires a flavor before the step can pass', () => {
    expect(flavorStepSchema().safeParse({ flavor: '' }).success).toBe(false);
    expect(flavorStepSchema().safeParse({ flavor: 'fabric' }).success).toBe(
      true,
    );
  });
});

describe('versionStepSchema', () => {
  it('requires a game version', () => {
    expect(
      versionStepSchema().safeParse({ version: '', loaderVersion: '' }).success,
    ).toBe(false);
  });

  it('leaves the loader build optional', () => {
    expect(
      versionStepSchema().safeParse({ version: '1.21.1', loaderVersion: '' })
        .success,
    ).toBe(true);
  });
});

describe('detailsStepSchema', () => {
  const memory = { min: 2, max: 16 };
  const rules: KeyRule[] = [
    { key: 'max-players', kind: { type: 'integer', min: 1, max: 100 } },
    { key: 'server-port', kind: { type: 'integer', min: 1, max: 65535 } },
  ];
  const server = detailsStepSchema('server', memory, rules);
  const instance = detailsStepSchema('instance', memory, rules);

  it('accepts a well-formed server', () => {
    expect(server.safeParse(details()).success).toBe(true);
  });

  it('holds memory inside the range the host can honour', () => {
    expect(server.safeParse(details({ memory: 1 })).success).toBe(false);
    expect(server.safeParse(details({ memory: 17 })).success).toBe(false);
    expect(server.safeParse(details({ memory: 2 })).success).toBe(true);
    expect(server.safeParse(details({ memory: 16 })).success).toBe(true);
  });

  it('holds players to a whole number inside the daemon’s range', () => {
    expect(server.safeParse(details({ maxPlayers: '' })).success).toBe(false);
    expect(server.safeParse(details({ maxPlayers: '1.5' })).success).toBe(false);
    expect(server.safeParse(details({ maxPlayers: '0' })).success).toBe(false);
    expect(server.safeParse(details({ maxPlayers: '101' })).success).toBe(false);
    expect(server.safeParse(details({ maxPlayers: '1' })).success).toBe(true);
  });

  it('checks the whole-number shape alone until the daemon’s rules arrive', () => {
    const unruled = detailsStepSchema('server', memory, []);
    expect(unruled.safeParse(details({ maxPlayers: '0' })).success).toBe(true);
    expect(unruled.safeParse(details({ maxPlayers: 'x' })).success).toBe(false);
  });

  it('treats an empty port as "let the daemon pick"', () => {
    expect(server.safeParse(details({ port: '' })).success).toBe(true);
  });

  it('keeps a given port inside the addressable range', () => {
    expect(server.safeParse(details({ port: '25565' })).success).toBe(true);
    expect(server.safeParse(details({ port: '65536' })).success).toBe(false);
    expect(server.safeParse(details({ port: 'abc' })).success).toBe(false);
  });

  it('requires the EULA for a server', () => {
    expect(server.safeParse(details({ eula: false })).success).toBe(false);
  });

  it('does not ask an instance to accept the EULA', () => {
    expect(instance.safeParse(details({ eula: false })).success).toBe(true);
  });
});
