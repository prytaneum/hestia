/**
 * `job.cancel` — one channel for every job family, since a job is identified
 * by the id its own events carry.
 */
import { cancelJob } from '../job';
import { type ChannelHandlers, str } from '../support';

export const channels: ChannelHandlers = {
  'job.cancel': (p) => ({ cancelled: cancelJob(str(p, 'id')) }),
};
