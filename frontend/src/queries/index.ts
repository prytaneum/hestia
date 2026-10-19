/**
 * The React Query layer over `../api`, one module per domain mirroring the
 * API namespaces 1:1. Each domain exports its `queryOptions`/mutation
 * factories (`serverQueries`, `serverMutations`, …) — usable directly in
 * router loaders and `useMutation` — plus named hooks only where a factory
 * cannot express the behaviour.
 * Cross-cutting pieces: the `queryClient` singleton, the key factory, the
 * global job store, the daemon-event invalidation feed, and the streaming
 * hooks (`useConnection`, `useDaemonEvent`, log following).
 */
export * from './accounts';
export * from './app';
export * from './cache';
export { catalogueQueries } from './catalogue';
export { invalidate, queryClient } from './client';
export * from './config';
export { useConnection } from './connection';
export * from './content';
export { type MutationSpec, mutation } from './core';
export * from './daemon';
export * from './entry';
export * from './entry-content';
export { useDaemonEvent } from './events';
export * from './files';
export * from './host';
export * from './instance';
export { startInvalidation } from './invalidation';
export * from './java';
export {
  type Job,
  type JobEntry,
  type JobHandle,
  type JobMeta,
  type JobMutationOptions,
  type JobMutationResult,
  type JobSpec,
  type JobStatus,
  jobMutation,
  startJob,
  startJobs,
  useBusyJob,
  useEntryJobs,
  useJobDisplay,
  useJobMutation,
  useJobs,
} from './jobs';
export { keys } from './keys';
export { type LogsOptions, type LogsResult, useFollowedLogs } from './logs';
export {
  type MetricSample,
  type ProcessMetricsResult,
  useProcessMetrics,
} from './metrics';
export * from './modpack';
export * from './net';
export * from './players';
export * from './prefs';
export * from './server';
export * from './skins';
export * from './sync';
export * from './tasks';
export * from './transfer';
export * from './update';
