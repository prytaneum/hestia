/**
 * `instance.*`. An instance is a lightweight record at create; its files
 * materialise during the `launch` job, which here means a supervised session
 * appearing and starting to broadcast. Content and settings come from ./entry,
 * which a server shares.
 */
import type {
  InstanceDetails,
  InstanceInfo,
  RecentPlay,
  ServerEntry,
} from '@/api/types';

import { jobIdOf, startJob } from '../job';
import * as catalog from '../state/catalog';
import * as entries from '../state/entries';
import * as processes from '../state/processes';
import * as worlds from '../state/worlds';
import { ago, bool, type Handlers, now, ok, str } from '../support';
import { resolve as resolveAccount } from './account';
import { configChannels } from './entry';

const BANNER = [
  '[main/INFO]: Setting user: Player',
  '[main/INFO]: Backend library: LWJGL version 3.3.3',
  '[Render thread/INFO]: OpenAL initialized on device Mock Output',
];

/** An instance record composed with its live sessions. */
const withSessions = (instance: InstanceInfo): InstanceInfo => ({
  ...instance,
  sessions: processes.sessionsOf(instance.id),
});

const instanceId = (payload: Record<string, unknown>): string =>
  entries.findInstance(str(payload, 'instance')).id;

const RECENT_LIMIT = 5;

/** Every played world, plus each instance's first listed server, newest first. */
function recentPlays(): RecentPlay[] {
  return entries
    .listInstances()
    .flatMap((instance): RecentPlay[] => {
      const base = { instance: instance.id, instanceName: instance.name };
      const played = worlds
        .worldsOf(instance.id)
        .filter((world) => world.lastPlayedUnix != null)
        .map((world) => ({
          ...base,
          target: { kind: 'world' as const, target: world.folder },
          name: world.name,
          gameMode: world.gameMode,
          playedUnix: world.lastPlayedUnix ?? 0,
          icon: world.icon,
        }));
      const joined = worlds
        .serversOf(instance.id)
        .slice(0, 1)
        .map((server) => ({
          ...base,
          target: { kind: 'server' as const, target: server.address },
          name: server.name,
          gameMode: 'survival' as const,
          playedUnix: ago(7_200),
          icon: server.icon,
        }));
      return [...played, ...joined];
    })
    .sort((a, b) => b.playedUnix - a.playedUnix)
    .slice(0, RECENT_LIMIT);
}

export const channels: Handlers = {
  'instance.list': () => ({
    instances: entries.listInstances().map(withSessions),
  }),

  'instance.info': (p): InstanceDetails => {
    const instance = entries.findInstance(str(p, 'instance'));
    return {
      ...instance,
      ...entries.directories(instance.id, 'instances'),
      diskBytes: 512 * 1024 * 1024,
    };
  },

  'instance.flavors': () => ({ flavors: catalog.instanceFlavors, rules: [] }),
  'instance.versions': (p) => ({
    versions: catalog.versionsFor(str(p, 'flavor')),
  }),
  'instance.loaders': (p) => ({ loaders: catalog.loaders(str(p, 'flavor')) }),

  'instance.create': (p) => ({
    instance: withSessions(
      entries.addInstance({
        name: str(p, 'name'),
        flavor: str(p, 'flavor', 'vanilla'),
        version: str(p, 'version'),
        loaderVersion: str(p, 'loaderVersion') || undefined,
      }),
    ),
  }),

  'instance.update': (p) => {
    const instance = entries.findInstance(str(p, 'instance'));
    instance.gameVersion = str(p, 'version', instance.gameVersion);
    instance.loaderVersion = str(p, 'loaderVersion') || instance.loaderVersion;
    instance.javaMajor = catalog.javaFor(instance.gameVersion);
    return { instance: withSessions(instance) };
  },

  'instance.rename': (p) =>
    withSessions(
      entries.rename(entries.findInstance(str(p, 'instance')), str(p, 'name')),
    ),

  'instance.remove': (p) => {
    entries.removeEntry(instanceId(p), entries.listInstances());
    return ok();
  },

  'instance.worlds': (p) => ({ worlds: worlds.worldsOf(instanceId(p)) }),

  'instance.recent': () => ({ plays: recentPlays() }),

  'instance.servers': (p) => ({ servers: worlds.serversOf(instanceId(p)) }),

  'instance.server.edit': (p) => {
    const id = instanceId(p);
    const entry: ServerEntry = {
      name: str(p, 'name'),
      address: str(p, 'address'),
      icon: '',
      acceptTextures: bool(p, 'acceptTextures'),
      hidden: false,
    };
    return {
      servers: worlds.editServer(id, str(p, 'server'), entry),
      warnings: [],
    };
  },

  'instance.server.remove': (p) => ({
    servers: worlds.removeServer(instanceId(p), str(p, 'server')),
    warnings: [],
  }),

  'minecraft.ping': (p) => ({
    playersOnline: 3,
    playersMax: 20,
    motd: `A Mock Server — ${str(p, 'address', 'localhost')}`,
    version: '1.21.4',
  }),

  'instance.launch': (p) => {
    const instance = entries.findInstance(str(p, 'instance'));
    const account = resolveAccount(str(p, 'account'));
    return startJob({
      id: jobIdOf(p, 'instance-launch'),
      channel: 'instance.launch',
      steps: [
        { phase: 'resolving', detail: instance.gameVersion },
        { phase: 'java', detail: `Java ${instance.javaMajor}` },
        { phase: 'libraries', detail: 'libraries' },
        { phase: 'assets', detail: 'assets' },
        { phase: 'client', detail: 'client jar' },
      ],
      done: () => {
        const id = processes.sessionId(instance.id);
        const session = processes.start(
          id,
          `${entries.HOME}/java/21/bin/java`,
          ['-Xmx6G', 'net.minecraft.client.main.Main'],
          [`[main/INFO]: Setting user: ${account.name}`, ...BANNER.slice(1)],
        );
        instance.lastPlayedUnix = now();
        return { processId: session.id, pid: session.pid, warnings: [] };
      },
    });
  },

  'instance.stop': (p) => {
    processes.stopSessions(instanceId(p), str(p, 'session') || undefined);
    return ok();
  },

  'instance.logs': (p) => {
    const id = instanceId(p);
    const session =
      str(p, 'session') || processes.sessionsOf(id).at(-1)?.id || '';
    return { lines: processes.logs(session, p.tail as number | undefined) };
  },

  ...configChannels('instance', instanceId),
};
