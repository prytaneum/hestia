/**
 * `job.cancel` — one channel for every job family, since a job is identified
 * by the id its own events carry.
 */
import { cancelJob, listJobs } from '../job';
import { type ChannelHandlers, str } from '../support';

export const channels: ChannelHandlers = {
  'job.list': listJobs,
  'job.cancel': (p) => ({ cancelled: cancelJob(str(p, 'id')) }),
};
