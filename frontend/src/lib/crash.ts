import { invoke } from '@tauri-apps/api/core';

import { log } from './log';

function detailOf(error: unknown): string {
  if (error instanceof Error) {
    return error.stack ?? `${error.name}: ${error.message}`;
  }
  try {
    return JSON.stringify(error, null, 2);
  } catch {
    return String(error);
  }
}

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

export function report(kind: string, error: unknown, location = ''): void {
  log.error({ kind, location }, messageOf(error));
  invoke('crash_report', {
    kind,
    message: messageOf(error),
    location,
    detail: detailOf(error),
  }).catch(() => {});
}

export function installCrashHandlers(): void {
  window.addEventListener('error', (event) => {
    const where = event.filename
      ? `${event.filename}:${event.lineno}:${event.colno}`
      : '';
    report('ui-error', event.error ?? event.message, where);
  });

  window.addEventListener('unhandledrejection', (event) => {
    report('ui-rejection', event.reason);
  });
}
