/**
 * `java.*` — the installed runtimes and the Adoptium releases they come from.
 * An install is a job with its own progress shape (install phases rather than
 * provisioning steps), so it runs the plan directly.
 */
import type { JavaInstallPhase, JavaRelease, JavaRuntime } from '@/api/types';

import { jobIdOf, runPlan } from '../job';
import { HOME } from '../state/entries';
import { type ChannelHandlers, num, ok } from '../support';

const releases: JavaRelease[] = [
  { major: 25, lts: false },
  { major: 21, lts: true },
  { major: 17, lts: true },
  { major: 11, lts: true },
  { major: 8, lts: true },
];

const runtime = (major: number, inUse: boolean): JavaRuntime => ({
  vendor: 'Eclipse Temurin',
  major,
  releaseName: `jdk-${major}.0.5+11`,
  home: `${HOME}/java/temurin-${major}`,
  executable: `${HOME}/java/temurin-${major}/bin/java`,
  inUse,
});

const runtimes: JavaRuntime[] = [runtime(21, true), runtime(17, false)];

const PHASES: JavaInstallPhase[] = ['resolving', 'downloading', 'extracting'];

export const channels: ChannelHandlers = {
  'java.releases': () => releases,
  'java.list': () => runtimes,

  'java.install': (p) => {
    const major = num(p, 'major', 21);
    const already = runtimes.some((entry) => entry.major === major);
    return runPlan({
      id: jobIdOf(p, 'java-install'),
      channel: 'java.install',
      ticks: PHASES.map((phase, index) => ({
        phase,
        current: index + 1,
        total: PHASES.length,
      })),
      done: () => {
        const installed = runtime(major, false);
        if (!already) runtimes.push(installed);
        return { runtime: installed, alreadyInstalled: already };
      },
    });
  },

  'java.uninstall': (p) => {
    const major = num(p, 'major');
    const at = runtimes.findIndex((entry) => entry.major === major);
    if (at >= 0) runtimes.splice(at, 1);
    return ok();
  },
};
