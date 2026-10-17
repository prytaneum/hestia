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
import type { ProvisionPhase } from '@/api/types';

import { publish } from './bus';
import { str } from './support';

/** How long one tick takes. Long enough to see, short enough to not wait. */
const TICK_MS = 260;

const running = new Map<string, () => void>();

interface Plan {
  /** The client's job id; every event of the job carries it. */
  id: string;
  /** The start channel — `server.create`, `content.add`, `java.install`, … */
  channel: string;
  /** One progress payload per tick. */
  ticks: Record<string, unknown>[];
  /** The done event's payload, minus its `id`, built as the job settles. */
  done: () => Record<string, unknown>;
}

/**
 * Run a plan and answer the way a daemon job channel does — with its id. A job
 * whose id is already running is answered, not started twice.
 */
export function runPlan(plan: Plan): { id: string } {
  if (running.has(plan.id)) return { id: plan.id };

  let tick = 0;
  let timer = 0;
  const settle = () => {
    window.clearInterval(timer);
    running.delete(plan.id);
  };

  timer = window.setInterval(() => {
    if (tick < plan.ticks.length) {
      publish('job', {
        id: plan.id,
        channel: plan.channel,
        state: 'progress',
        ...plan.ticks[tick],
      });
      tick += 1;
      return;
    }
    settle();
    publish('job', {
      id: plan.id,
      channel: plan.channel,
      state: 'done',
      ...plan.done(),
    });
  }, TICK_MS);

  running.set(plan.id, () => {
    settle();
    publish('job', {
      id: plan.id,
      channel: plan.channel,
      state: 'cancelled',
    });
  });
  return { id: plan.id };
}

/** One step of a provisioning job — a phase, and what the UI names under it. */
interface JobStep {
  phase: ProvisionPhase;
  detail?: string;
}

/**
 * A provisioning job: the `ProvisionProgress` shape most jobs publish
 * (`current`/`total` are the step counts a phase with unknown extent reports).
 */
export function startJob(plan: {
  id: string;
  channel: string;
  steps: JobStep[];
  done: () => Record<string, unknown>;
}): { id: string } {
  const total = plan.steps.length;
  return runPlan({
    id: plan.id,
    channel: plan.channel,
    ticks: plan.steps.map((step, index) => ({
      phase: step.phase,
      current: index + 1,
      total,
      detail: step.detail ?? '',
      item: 0,
      items: 0,
    })),
    done: plan.done,
  });
}

/** `job.cancel`: false means it was already over — a race, not an error. */
export function cancelJob(id: string): boolean {
  const cancel = running.get(id);
  if (!cancel) return false;
  cancel();
  return true;
}

/** The id a job channel was given, or a minted one when the caller omitted it. */
export function jobIdOf(
  payload: Record<string, unknown>,
  family: string,
): string {
  return str(payload, 'id', `${family}-${Date.now().toString(36)}`);
}
