/**
 * The typed daemon API, one namespace per domain — the frontend's mirror of
 * the Rust client SDK's facades. Everything crosses the shell's generic
 * `ipc_call` bridge; channel names and payload shapes mirror `crates/proto`.
 */
export * as accounts from './accounts';
export * as app from './app';
export * as cache from './cache';
export * as config from './config';
export type { ContentAddInput } from './content';
export * as content from './content';
export { errorMessage, errorMessageFromInfo } from './core/errors';
export {
  type ConnectionState,
  type DaemonEvent,
  onConnectionChange,
  onDaemonEvent,
  onTopic,
} from './core/events';
export {
  BAD_REQUEST,
  CONNECTION_LOST,
  call,
  HANDLER_ERROR,
  HestiaError,
  isNotFound,
  NOT_FOUND,
  OFFLINE,
  TIMEOUT,
  TRANSPORT,
  tryCall,
  UNAUTHORIZED,
  UNKNOWN_CHANNEL,
} from './core/ipc';
export {
  JobCancelled,
  type JobParams,
  type JobRun,
  jobId,
  runJob,
} from './core/jobs';
export { warningHint, warningMessage } from './core/warnings';
export * as daemon from './daemon';
export * as dialog from './dialog';
export * as entry from './entry';
export * as files from './files';
export * as host from './host';
export * as instance from './instance';
export * as java from './java';
export * as job from './job';
export type { PackRef } from './modpack';
export * as modpack from './modpack';
export * as net from './net';
export * as players from './players';
export * as prefs from './prefs';
export * as screenshot from './screenshot';
export * as server from './server';
export * as skins from './skins';
export * as sync from './sync';
export * as system from './system';
export * as tasks from './tasks';
export type { ExportInput } from './transfer';
export * as transfer from './transfer';
export type * from './types';
export type { Channels, Jobs, Topics } from './types/channels';
export * as update from './update';
export { downgradeBetween } from './version';
