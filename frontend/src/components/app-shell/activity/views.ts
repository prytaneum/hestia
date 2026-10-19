import { useInstances } from '@/queries/instance';
import { type Job, jobTitle } from '@/queries/jobs';
import { useServers } from '@/queries/server';

export interface JobView {
  title: string;
  iconUrl?: string;
  /** The entry the job acts on is still in the library, so it can be opened. */
  openable: boolean;
}

/** Resolves a job to what Activity shows for it, from the cached entry lists. */
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
    return {
      title: jobTitle(job, entry?.name),
      iconUrl: entry?.iconUrl,
      openable: entry !== undefined,
    };
  };
}
