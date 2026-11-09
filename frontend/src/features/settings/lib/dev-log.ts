export interface DevLogLine {
  level: string;
  text: string;
}

export const MAX_DEV_LOG_LINES = 500;

/** The newest `max` lines: the log is a live tail, not a history. */
export function appendDevLog(
  lines: DevLogLine[],
  line: DevLogLine,
  max = MAX_DEV_LOG_LINES,
): DevLogLine[] {
  const next = [...lines, line];
  return next.length > max ? next.slice(next.length - max) : next;
}
