/**
 * The fixture job driver. A long-running daemon operation answers its start
 * call immediately and settles later on the `job` topic; the client's `runJob`
 * blocks until a terminal event arrives, so a job that never publishes one
 * hangs the caller forever. This walks a plan on a timer, publishes a progress
 * event per tick, and settles — cancellably, since `job.cancel` must be able to
 * reach it.
 *
 * `done` is a thunk rather than a value: what a done event reports (the entry
 * a create minted, the pack an install landed) is mutated when the job
 * finishes, not when it starts.
 */
import type { JobInfo, JobSubject, Jobs, ProvisionPhase } from '@/api';

import { publishJob } from './bus';
import { now, str } from './support';

/** How long one tick takes. Long enough to see, short enough to not wait. */
const TICK_MS = 260;

/** The daemon's job registry: what `job.list` reports until a job settles. */
const running = new Map<string, { info: JobInfo; cancel: () => void }>();

export const subjectOf = (
  kind: JobSubject['kind'],
  id: string,
): JobSubject => ({ kind, id });

interface Plan<K extends keyof Jobs> {
  /** The client's job id; every event of the job carries it. */
  id: string;
  /** The start channel — `server.create`, `content.add`, `java.install`, … */
  channel: K;
  /** The entry the job acts on, when it acts on one. */
  subject?: JobSubject;
  /** One progress payload per tick. */
  ticks: Jobs[K]['progress'][];
  /** The done event's payload, built as the job settles. */
  done: () => Jobs[K]['done'];
}

/**
 * Run a plan and answer the way a daemon job channel does — with its id. A job
 * whose id is already running is answered, not started twice.
 */
export function runPlan<K extends keyof Jobs>(plan: Plan<K>): { id: string } {
  if (running.has(plan.id)) return { id: plan.id };

  let tick = 0;
  let timer = 0;
  const settle = () => {
    window.clearInterval(timer);
    running.delete(plan.id);
  };

  const info: JobInfo = {
    id: plan.id,
    channel: plan.channel,
    subject: plan.subject,
    startedUnix: now(),
    progress: null,
  };

  timer = window.setInterval(() => {
    if (tick < plan.ticks.length) {
      info.progress = plan.ticks[tick];
      publishJob(
        plan.id,
        plan.channel,
        { state: 'progress', ...plan.ticks[tick] },
        plan.subject,
      );
      tick += 1;
      return;
    }
    settle();
    publishJob(
      plan.id,
      plan.channel,
      { state: 'done', ...plan.done() },
      plan.subject,
    );
  }, TICK_MS);

  running.set(plan.id, {
    info,
    cancel: () => {
      settle();
      publishJob(plan.id, plan.channel, { state: 'cancelled' }, plan.subject);
    },
  });
  return { id: plan.id };
}

/** `job.list`: every job that has not settled. */
export const listJobs = (): JobInfo[] =>
  [...running.values()].map(({ info }) => info);

/** One step of a provisioning job — a phase, what the UI names under it, and the bytes it fetches. */
interface JobStep {
  phase: ProvisionPhase;
  detail?: string;
  bytes?: number;
}

type ProvisionJob = {
  [K in keyof Jobs]: Jobs[K]['progress'] extends { phase: ProvisionPhase }
    ? K
    : never;
}[keyof Jobs];

/**
 * A provisioning job: the `ProvisionProgress` shape most jobs publish
 * (`current`/`total` are the step counts a phase with unknown extent reports).
 */
export function startJob<K extends ProvisionJob>(plan: {
  id: string;
  channel: K;
  subject?: JobSubject;
  steps: JobStep[];
  done: () => Jobs[K]['done'];
}): { id: string } {
  const total = plan.steps.length;
  const overallTotal = plan.steps.reduce((sum, s) => sum + (s.bytes ?? 0), 0);
  let overallCurrent = 0;
  return runPlan({
    id: plan.id,
    channel: plan.channel,
    subject: plan.subject,
    ticks: plan.steps.map((step, index) => {
      overallCurrent += step.bytes ?? 0;
      return {
        phase: step.phase,
        current: step.bytes ?? index + 1,
        total: step.bytes ?? total,
        detail: step.detail ?? '',
        item: 0,
        items: 0,
        overallCurrent,
        overallTotal,
      };
    }),
    done: plan.done,
  });
}

/** `job.cancel`: false means it was already over — a race, not an error. */
export function cancelJob(id: string): boolean {
  const job = running.get(id);
  if (!job) return false;
  job.cancel();
  return true;
}

/** The id a job channel was given, or a minted one when the caller omitted it. */
export function jobIdOf(
  payload: Record<string, unknown>,
  family: string,
): string {
  return str(payload, 'id', `${family}-${Date.now().toString(36)}`);
}
