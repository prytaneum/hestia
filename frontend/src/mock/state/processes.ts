/**
 * The fixture supervisor. A started server or game session is a record here,
 * and — because the desktop reads running state from events, not from polling
 * — it also broadcasts: `process.started` on spawn, `process.metrics` and
 * `process.output` while it runs, `process.exit` when it is stopped.
 *
 * Process ids follow the daemon's own convention, which the invalidation feed
 * and the window tracker both parse: `server-<id>` and `instance-<id>_<seq>`.
 */
import type { ProcessInfo, ProcessLogLine } from '@/api/types';
import {
  instanceSessionId,
  instanceSessionPrefix,
  serverProcessId,
} from '@/lib/process-id';

import { publish } from '../bus';
import { now } from '../support';

const METRICS_MS = 2_000;
const MAX_LINES = 400;

interface Supervised {
  info: ProcessInfo;
  lines: ProcessLogLine[];
  cpu: number;
  mem: number;
  tick: number;
}

const supervised = new Map<string, Supervised>();
let ticker = 0;
let nextPid = 4_300;

/** The next free session id of an instance — sessions are numbered from 1. */
export function sessionId(instanceId: string): string {
  const taken = sessionsOf(instanceId).length;
  return instanceSessionId(instanceId, taken + 1);
}

export const get = (id: string): ProcessInfo | undefined =>
  supervised.get(id)?.info;

export const list = (): ProcessInfo[] =>
  [...supervised.values()].map((entry) => entry.info);

export function sessionsOf(instanceId: string): ProcessInfo[] {
  const prefix = instanceSessionPrefix(instanceId);
  return list().filter(
    (info) => info.id.startsWith(prefix) && info.state === 'running',
  );
}

export function serverProcess(id: string): ProcessInfo | undefined {
  const info = get(serverProcessId(id));
  return info?.state === 'running' ? info : undefined;
}

export const isRunning = (id: string): boolean => get(id)?.state === 'running';

export function logs(id: string, tail?: number): ProcessLogLine[] {
  const lines = supervised.get(id)?.lines ?? [];
  return tail && tail > 0 ? lines.slice(-tail) : lines;
}

/** Spawn a process, announce it, and start its output and metrics streams. */
export function start(
  id: string,
  program: string,
  args: string[],
  banner: string[],
): ProcessInfo {
  const existing = supervised.get(id);
  if (existing?.info.state === 'running') return existing.info;

  nextPid += 1;
  const info: ProcessInfo = {
    id,
    pid: nextPid,
    program,
    args,
    state: 'running',
    startedUnix: now(),
  };
  const lines: ProcessLogLine[] = [];
  for (const line of banner) lines.push(written(lines, line));
  supervised.set(id, {
    info,
    lines,
    cpu: 18,
    mem: 1_400 * 1024 * 1024,
    tick: 0,
  });
  publish('process.started', { id, pid: info.pid });
  ensureTicker();
  return info;
}

export function stop(id: string): void {
  const entry = supervised.get(id);
  if (entry?.info.state !== 'running') return;
  entry.info.state = 'exited';
  entry.info.exitCode = 0;
  append(id, 'Stopping the server');
  publish('process.exit', { id, state: 'exited', exitCode: 0, success: true });
  ensureTicker();
}

/** Stop every session of an instance, or the one session named. */
export function stopSessions(instanceId: string, session?: string): void {
  if (session) {
    stop(session);
    return;
  }
  for (const info of sessionsOf(instanceId)) stop(info.id);
}

/** Append a line to a process's captured log and stream it to followers. */
export function append(id: string, line: string): void {
  const entry = supervised.get(id);
  if (!entry) return;
  const record = written(entry.lines, line);
  entry.lines.push(record);
  if (entry.lines.length > MAX_LINES) entry.lines.shift();
  publish('process.output', { id, lines: [record] });
}

function written(log: ProcessLogLine[], line: string): ProcessLogLine {
  const last = log.at(-1);
  return {
    stream: 'stdout',
    line: stamp(line),
    offset: last ? last.offset + last.line.length + 1 : 0,
  };
}

function stamp(line: string): string {
  const time = new Date().toTimeString().slice(0, 8);
  return `[${time}] [Server thread/INFO]: ${line}`;
}

const CHATTER = [
  'Ari joined the game',
  'Ari[/127.0.0.1:52114] logged in with entity id 214 at (118.5, 71.0, -304.2)',
  'Saving chunks for level "world"/minecraft:overworld',
  'Kai joined the game',
  '<Ari> anyone got spare iron',
  'Saved the game',
  '<Kai> heading to the mine, brb',
  'Kai lost connection: Disconnected',
  'Kai left the game',
];

/**
 * One timer drives every running process: a metrics broadcast each tick (the
 * overview charts read it) and a log line every fourth, so a console that is
 * open has something arriving in it.
 */
function ensureTicker(): void {
  const running = [...supervised.values()].filter(
    (entry) => entry.info.state === 'running',
  );
  if (running.length === 0) {
    window.clearInterval(ticker);
    ticker = 0;
    return;
  }
  if (ticker !== 0) return;

  ticker = window.setInterval(() => {
    const live = [...supervised.values()].filter(
      (entry) => entry.info.state === 'running',
    );
    if (live.length === 0) return ensureTicker();

    for (const entry of live) {
      entry.tick += 1;
      entry.cpu = clamp(entry.cpu + (Math.random() - 0.5) * 12, 2, 96);
      entry.mem = clamp(
        entry.mem + (Math.random() - 0.5) * 96 * 1024 * 1024,
        512 * 1024 * 1024,
        6 * 1024 * 1024 * 1024,
      );
      if (entry.tick % 4 === 0)
        append(entry.info.id, CHATTER[entry.tick % CHATTER.length]);
    }

    publish('process.metrics', {
      samples: live.map((entry) => ({
        id: entry.info.id,
        cpuPct: Math.round(entry.cpu * 10) / 10,
        memBytes: Math.round(entry.mem),
      })),
    });
  }, METRICS_MS);
}

const clamp = (value: number, min: number, max: number): number =>
  Math.min(max, Math.max(min, value));
