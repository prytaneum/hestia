/**
 * `server.*`. Unlike an instance, a server is fully provisioned at create —
 * so `create` and `update` are jobs, and everything after them reads a record
 * that is already `ready`. Backups live in ./backup; settings and content come
 * from ./entry.
 */
import type {
  ConfigEntry,
  ServerDetails,
  ServerInfo,
  ServerProperty,
} from '@/api/types';

import { jobIdOf, startJob } from '../job';
import * as catalog from '../state/catalog';
import * as entries from '../state/entries';
import * as processes from '../state/processes';
import { type Handlers, num, ok, str } from '../support';
import { channels as backupChannels } from './backup';
import { configChannels } from './entry';

// The daemon's own knowledge of the file, which no front-end shares: what it
// keeps on the record instead, what it owns, and what hardcore pins.
const RECORD_KEYS = [
  'memory',
  'jvm-args',
  'backup-interval',
  'backup-retention',
];
const MANAGED = ['server-port', 'enable-rcon', 'rcon.port', 'rcon.password'];
const SECRET = ['rcon.password'];
const HARDCORE_PINNED = ['difficulty', 'gamemode'];

function properties(settings: ConfigEntry[]): ServerProperty[] {
  const file = settings.filter((entry) => !RECORD_KEYS.includes(entry.key));
  const hardcore = file.some((e) => e.key === 'hardcore' && e.value === 'true');
  return file
    .map(({ key, value }) => {
      const secret = SECRET.includes(key);
      return {
        key,
        value: secret ? '' : value,
        default: secret ? undefined : value,
        managed: MANAGED.includes(key),
        secret,
        locked: hardcore && HARDCORE_PINNED.includes(key),
      };
    })
    .sort((a, b) => a.key.localeCompare(b.key));
}

/** What a vanilla server prints on the way up, down to the RCON listener. */
const banner = (server: ServerInfo): string[] => [
  `Starting minecraft server version ${server.gameVersion}`,
  'Loading properties',
  'Default game type: SURVIVAL',
  'Generating keypair',
  `Starting Minecraft server on *:${server.gamePort ?? 25_565}`,
  'Preparing level "world"',
  'Preparing start region for dimension minecraft:overworld',
  'Time elapsed: 1275 ms',
  'Done (3.412s)! For help, type "help"',
  'Starting remote control listener',
  'Thread RCON Listener started',
];

/** The handful of vanilla commands the fixture answers over RCON. */
const RCON: Record<string, (args: string) => string> = {
  say: (args) => `[Server] ${args}`,
  list: () => 'There are 2 of a max of 20 players online: Ari, Kai',
  seed: () => 'Seed: [-4707391018842391]',
  time: () => 'The time is 13240',
  'save-all': () => 'Saved the game',
  help: () => 'Try: list, say <message>, seed, time, save-all.',
};

/** A server record composed with its live process. */
const withProcess = (server: ServerInfo): ServerInfo => ({
  ...server,
  process: processes.serverProcess(server.id),
});

const serverId = (payload: Record<string, unknown>): string =>
  entries.findServer(str(payload, 'server')).id;

function startServer(reference: string) {
  const server = entries.findServer(reference);
  const process = processes.start(
    processes.serverProcessId(server.id),
    `${entries.HOME}/java/${server.javaMajor}/bin/java`,
    ['-Xmx4G', '-jar', 'server.jar', 'nogui'],
    banner(server),
  );
  return { processId: process.id, pid: process.pid };
}

export const channels: Handlers = {
  'server.list': () => ({ servers: entries.listServers().map(withProcess) }),

  'server.status': (p) => withProcess(entries.findServer(str(p, 'server'))),

  'server.info': (p): ServerDetails => {
    const server = entries.findServer(str(p, 'server'));
    return {
      ...server,
      ...entries.directories(server.id, 'servers'),
      diskBytes: 1_024 * 1024 * 1024,
      jvmArgs: ['-XX:+UseG1GC'],
      jvmArgsSource: 'defaults',
      warnings: [],
    };
  },

  'server.ping': (p) => {
    const server = entries.findServer(str(p, 'server'));
    return {
      playersOnline: processes.serverProcess(server.id) ? 2 : 0,
      playersMax: 20,
      motd: `${server.name} — a ${server.flavor} ${server.gameVersion} server`,
      version: server.gameVersion,
    };
  },

  'server.flavors': () => ({
    flavors: catalog.serverFlavors,
    rules: catalog.serverRules,
  }),
  'server.versions': (p) => ({
    versions: catalog.versionsFor(str(p, 'flavor')),
  }),
  'server.loaders': (p) => ({ loaders: catalog.loaders(str(p, 'flavor')) }),

  'server.create': (p) =>
    startJob({
      id: jobIdOf(p, 'server-create'),
      channel: 'server.create',
      steps: [
        { phase: 'resolving', detail: str(p, 'version') },
        { phase: 'java', detail: 'Java runtime' },
        { phase: 'server', detail: 'server jar' },
        { phase: 'overrides', detail: 'eula.txt' },
      ],
      done: () => ({
        server: withProcess(
          entries.addServer({
            name: str(p, 'name'),
            flavor: str(p, 'flavor', 'vanilla'),
            version: str(p, 'version'),
            loaderVersion: str(p, 'loaderVersion') || undefined,
            port: num(p, 'port') || undefined,
          }),
        ),
        warnings: [],
      }),
    }),

  'server.update': (p) => {
    const server = entries.findServer(str(p, 'server'));
    return startJob({
      id: jobIdOf(p, 'server-update'),
      channel: 'server.update',
      steps: [
        { phase: 'backup', detail: 'backing up first' },
        { phase: 'resolving', detail: str(p, 'version') },
        { phase: 'server', detail: 'server jar' },
      ],
      done: () => {
        server.gameVersion = str(p, 'version', server.gameVersion);
        server.loaderVersion = str(p, 'loaderVersion') || server.loaderVersion;
        server.javaMajor = catalog.javaFor(server.gameVersion);
        return { server: withProcess(server), warnings: [] };
      },
    });
  },

  'server.rename': (p) =>
    withProcess(
      entries.rename(entries.findServer(str(p, 'server')), str(p, 'name')),
    ),

  'server.remove': (p) => {
    entries.removeEntry(serverId(p), entries.listServers());
    return ok();
  },

  'server.start': (p) => startServer(str(p, 'server')),

  'server.restart': (p) => {
    processes.stop(processes.serverProcessId(serverId(p)));
    return startServer(str(p, 'server'));
  },

  'server.stop': (p) => {
    processes.stop(processes.serverProcessId(serverId(p)));
    return ok();
  },

  'server.logs': (p) => ({
    lines: processes.logs(
      processes.serverProcessId(serverId(p)),
      p.tail as number | undefined,
    ),
  }),

  // One RCON round trip. Only a running server answers — the console tab
  // renders the rejection, so refusing here is the honest response.
  'server.command': (p) => {
    const id = serverId(p);
    if (!processes.serverProcess(id))
      throw { code: 'handler_error', message: 'the server is not running' };
    const command = str(p, 'command').trim();
    const [verb, ...rest] = command.split(/\s+/);
    const response = RCON[verb]?.(rest.join(' '));
    return {
      response: response ?? `Unknown or incomplete command: ${command}`,
    };
  },

  'server.command.complete': (p) => {
    const [typed, ...rest] = str(p, 'line').trimStart().split(/\s+/);
    const first = rest.length === 0;
    return {
      completions: first
        ? Object.keys(RCON).filter((verb) => verb.startsWith(typed))
        : [],
      arguments: !first && typed === 'say' ? ['<message>'] : [],
    };
  },

  'server.properties': (p) => ({
    properties: properties(entries.entrySettings(serverId(p))),
  }),

  ...configChannels('server', serverId),
  ...backupChannels(serverId),
};
