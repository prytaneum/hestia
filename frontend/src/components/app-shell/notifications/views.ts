import { useInstances } from '@/queries/instance';
import { type Job, jobTitle } from '@/queries/jobs';
import { useServers } from '@/queries/server';

export interface JobView {
  title: string;
}

/** Resolves a job to what the bell's panel shows for it, from the cached entry lists. */
export function useJobViews(): (job: Job) => JobView {
  const instances = useInstances().data;
  const servers = useServers().data;
  return (job) => {
    const { subject } = job;
    const entries =
      subject?.kind === 'instance'
        ? instances
        : subject?.kind === 'server'
          ? servers
          : undefined;
    const entry = entries?.find((candidate) => candidate.id === subject?.id);
    return { title: jobTitle(job, entry?.name) };
  };
}
