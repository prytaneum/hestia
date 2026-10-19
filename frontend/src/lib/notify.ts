import { toast } from 'sonner';
import {
  errorMessage,
  JobCancelled,
  JobInterrupted,
  type WarningInfo,
  warningHint,
  warningMessage,
} from '@/api';
import { m } from '@/paraglide/messages.js';

const SUCCESS_MS = 6_000;
const WARNING_MS = 8_000;

export type NotifyAction = { label: string; onClick: () => void };

export type NotifyOptions = {
  id?: string | number;
  description?: string;
  action?: NotifyAction;
};

export type Outcome = {
  id: string;
  title: string;
  description?: string;
  action?: NotifyAction;
  warnings?: WarningInfo[];
  failures?: { failed: number; total?: number };
};

type Kind = 'success' | 'info' | 'warning' | 'error';

const DURATIONS: Record<Kind, number> = {
  success: SUCCESS_MS,
  info: SUCCESS_MS,
  warning: WARNING_MS,
  error: Number.POSITIVE_INFINITY,
};

function show(kind: Kind, title: string, options: NotifyOptions = {}) {
  const { action, ...rest } = options;
  return toast[kind](title, {
    ...rest,
    closeButton: true,
    duration: action ? Number.POSITIVE_INFINITY : DURATIONS[kind],
    action: action && { label: action.label, onClick: action.onClick },
  });
}

function outcomeDescription({ description, warnings = [], failures }: Outcome) {
  const parts: string[] = description ? [description] : [];
  if (failures && failures.failed > 0) {
    parts.push(
      failures.total === undefined
        ? m['app.toast.failed_only']({ failed: failures.failed })
        : m['app.toast.failed_count']({
            failed: failures.failed,
            total: failures.total,
          }),
    );
  }
  if (warnings.length === 1) parts.push(warningMessage(warnings[0]));
  else if (warnings.length > 1) {
    parts.push(m['app.toast.warning_count']({ count: warnings.length }));
  }
  return parts.join(' · ') || undefined;
}

export const notify = {
  success: (title: string, options?: NotifyOptions) =>
    show('success', title, options),
  info: (title: string, options?: NotifyOptions) =>
    show('info', title, options),
  warning: (title: string, options?: NotifyOptions) =>
    show('warning', title, options),
  error: (title: string, options?: NotifyOptions) =>
    show('error', title, options),

  /** One toast for a finished operation, its warnings and failed items folded in. */
  outcome(outcome: Outcome) {
    const degraded =
      (outcome.warnings?.length ?? 0) > 0 ||
      (outcome.failures?.failed ?? 0) > 0;
    return show(degraded ? 'warning' : 'success', outcome.title, {
      id: outcome.id,
      description: outcomeDescription(outcome),
      action: outcome.action,
    });
  },

  /** A degraded-but-done write: all its warnings as one toast, never one each. */
  warnings(warnings: WarningInfo[] | undefined, { id }: { id: string }) {
    const [first, ...rest] = warnings ?? [];
    if (!first) return;
    const parts = [warningHint(first)];
    if (rest.length > 0) {
      parts.push(m['app.toast.warning_count']({ count: rest.length + 1 }));
    }
    return show('warning', warningMessage(first), {
      id,
      description: parts.filter(Boolean).join(' · ') || undefined,
    });
  },

  /** A cancelled or interrupted job is not a failure and shows nothing. */
  failure(
    error: unknown,
    {
      id,
      title,
      action,
    }: { id: string; title?: string; action?: NotifyAction },
  ) {
    if (error instanceof JobCancelled || error instanceof JobInterrupted) {
      return;
    }
    const message = errorMessage(error);
    return show('error', title ?? message, {
      id,
      description: title ? message : undefined,
      action,
    });
  },

  /** Loading under one id, replaced in place by the settled toast. */
  async promise<T>(
    promise: Promise<T>,
    options: {
      id: string;
      loading: string;
      success: (value: T) => string;
      error?: string;
    },
  ): Promise<T> {
    const { id, loading } = options;
    toast.loading(loading, { id, closeButton: true });
    try {
      const value = await promise;
      show('success', options.success(value), { id });
      return value;
    } catch (error) {
      toast.dismiss(id);
      notify.failure(error, { id, title: options.error });
      throw error;
    }
  },
};
