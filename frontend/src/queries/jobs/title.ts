import { m } from '@/paraglide/messages.js';
import type { Job } from './store';

const msg = m as unknown as Record<
  string,
  (params?: Record<string, unknown>) => string
>;

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
