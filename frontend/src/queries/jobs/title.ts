import { m } from '@/paraglide/messages.js';
import { queryClient } from '../client';
import { keys } from '../keys';
import type { Job, JobEntry } from './store';

const msg = m as unknown as Record<
  string,
  (params?: Record<string, unknown>) => string
>;

/** The display name of an entry, read off the cached list; absent until it loads. */
export function entryName(subject: JobEntry | null): string | undefined {
  if (!subject) return undefined;
  const list = queryClient.getQueryData<{ id: string; name: string }[]>(
    subject.kind === 'server' ? keys.servers.list() : keys.instances.list(),
  );
  return list?.find((entry) => entry.id === subject.id)?.name;
}

/**
 * The localized title of a job: `app.jobs.kind.<channel>` with the entry's
 * name, or its generic noun when the caller does not know the name.
 */
export function jobTitle(job: Job, entryName?: string): string {
  const name =
    entryName ??
    (job.subject ? msg[`domain.entry_type.${job.subject.kind}`]() : '');
  return msg[`app.jobs.kind.${job.channel}`]?.({ name }) ?? job.channel;
}

/** The entry's name for a toast, or its generic noun while the list is unloaded. */
export function subjectName(subject: JobEntry): string {
  return entryName(subject) ?? msg[`domain.entry_type.${subject.kind}`]();
}
