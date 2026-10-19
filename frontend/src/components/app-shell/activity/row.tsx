import {
  CheckCircleIcon,
  CubeIcon,
  DownloadSimpleIcon,
  HardDrivesIcon,
  type Icon,
  MinusCircleIcon,
  WarningIcon,
  XCircleIcon,
  XIcon,
} from '@phosphor-icons/react';
import { useNavigate } from '@tanstack/react-router';

import { errorMessage, type ProvisionProgress } from '@/api';
import { Button } from '@/components/ui/button';
import { Thumbnail } from '@/components/ui/thumbnail';
import { ProvisionProgressView } from '@/features/shared/entry/components';
import { agoLabel } from '@/lib/format';
import { outcomeDescription } from '@/lib/notify';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import {
  cancelJob,
  dismissJob,
  type Job,
  retryJob,
  setActivityOpen,
} from '@/queries/jobs';
import type { JobView } from './views';

const SUBJECT_ICONS = { server: HardDrivesIcon, instance: CubeIcon } as const;

const SETTLED: Record<
  Exclude<Job['status'], 'running'>,
  { icon: Icon; tone: string; label: () => string }
> = {
  done: {
    icon: CheckCircleIcon,
    tone: 'text-ember',
    label: () => m['app.jobs.panel.status_done'](),
  },
  failed: {
    icon: XCircleIcon,
    tone: 'text-destructive',
    label: () => m['app.jobs.panel.status_failed'](),
  },
  cancelled: {
    icon: MinusCircleIcon,
    tone: 'text-muted-foreground',
    label: () => m['app.jobs.panel.status_cancelled'](),
  },
  interrupted: {
    icon: WarningIcon,
    tone: 'text-amber',
    label: () => m['app.jobs.panel.status_interrupted'](),
  },
};

function detailOf(job: Job): string | undefined {
  if (job.status === 'failed' && job.error) return errorMessage(job.error);
  if (job.status !== 'done' || !job.outcome) return undefined;
  const { title, ...rest } = job.outcome;
  return [title, outcomeDescription(rest)].filter(Boolean).join(' · ');
}

export function ActivityRow({
  job,
  view,
  onRemoved,
}: {
  job: Job;
  view: JobView;
  onRemoved: () => void;
}) {
  return job.status === 'running' ? (
    <RunningRow job={job} view={view} />
  ) : (
    <SettledRow job={job} view={view} onRemoved={onRemoved} />
  );
}

function RunningRow({ job, view }: { job: Job; view: JobView }) {
  const SubjectIcon = job.subject
    ? SUBJECT_ICONS[job.subject.kind]
    : DownloadSimpleIcon;
  return (
    <li data-job-id={job.id} className="flex items-start gap-2.5 p-2">
      <Thumbnail src={view.iconUrl} icon={SubjectIcon} size="sm" />
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <span className="truncate text-xs font-medium text-foreground">
          {view.title}
        </span>
        <ProvisionProgressView
          compact
          progress={job.progress as ProvisionProgress | null}
        />
      </div>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={m['app.jobs.panel.cancel']({ title: view.title })}
        onClick={() => {
          cancelJob(job.id).catch(() => {});
        }}
      >
        <XIcon />
      </Button>
    </li>
  );
}

function SettledRow({
  job,
  view,
  onRemoved,
}: {
  job: Job;
  view: JobView;
  onRemoved: () => void;
}) {
  const navigate = useNavigate();
  const status = SETTLED[job.status as keyof typeof SETTLED];
  const StatusIcon = status.icon;
  const detail = detailOf(job);
  const subject = job.subject;

  const open = () => {
    if (!subject) return;
    setActivityOpen(false);
    if (subject.kind === 'server') {
      navigate({ to: '/servers/$id', params: { id: subject.id } });
    } else {
      navigate({ to: '/instances/$id', params: { id: subject.id } });
    }
  };

  const canRetry = job.status === 'failed' && job.retry !== null;
  const canOpen = subject !== null && view.openable;

  return (
    <li data-job-id={job.id} className="flex items-start gap-2.5 p-2">
      <StatusIcon
        aria-hidden
        weight="fill"
        className={cn('mt-0.5 size-4 shrink-0', status.tone)}
      />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="truncate text-xs font-medium text-foreground">
          {view.title}
        </span>
        <span className="text-[11px] text-muted-foreground">
          <span className={status.tone}>{status.label()}</span>
          {job.settledAt !== null && ` · ${agoLabel(job.settledAt / 1000)}`}
        </span>
        {detail && (
          <p className="text-[11px] break-words text-muted-foreground">
            {detail}
          </p>
        )}
        {(canRetry || canOpen) && (
          <div className="mt-1 flex gap-1">
            {canRetry && (
              <Button
                variant="outline"
                size="xs"
                aria-label={m['app.jobs.panel.retry_label']({
                  title: view.title,
                })}
                onClick={() => {
                  retryJob(job.id);
                  onRemoved();
                }}
              >
                {m['app.jobs.panel.retry']()}
              </Button>
            )}
            {canOpen && (
              <Button
                variant="outline"
                size="xs"
                aria-label={m['app.jobs.panel.open_label']({
                  title: view.title,
                })}
                onClick={open}
              >
                {m['app.jobs.panel.open']()}
              </Button>
            )}
          </div>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={m['app.jobs.panel.dismiss']({ title: view.title })}
        onClick={() => {
          dismissJob(job.id);
          onRemoved();
        }}
      >
        <XIcon />
      </Button>
    </li>
  );
}
