import {
  CheckCircleIcon,
  type Icon,
  InfoIcon,
  WarningIcon,
  XCircleIcon,
  XIcon,
} from '@phosphor-icons/react';
import { useRef } from 'react';

import type { ProvisionProgress } from '@/api';
import { Button } from '@/components/ui/button';
import { PopoverContent, PopoverTitle } from '@/components/ui/popover';
import { Progress } from '@/components/ui/progress';
import { Spinner } from '@/components/ui/spinner';
import {
  CancelJobButton,
  isMeasurable,
  overallRatio,
  phaseLabel,
} from '@/features/shared/entry/components';
import { agoLabel } from '@/lib/format';
import {
  clearNotices,
  type Notice,
  type NoticeKind,
  removeNotice,
} from '@/lib/notifications';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import type { Job } from '@/queries/jobs';
import type { JobView } from './views';

const ICONS: Record<NoticeKind, Icon> = {
  success: CheckCircleIcon,
  info: InfoIcon,
  warning: WarningIcon,
  error: XCircleIcon,
};

export function NotificationPanel({
  running,
  notices,
  viewOf,
}: {
  running: Job[];
  notices: Notice[];
  viewOf: (job: Job) => JobView;
}) {
  const titleRef = useRef<HTMLHeadingElement>(null);
  const empty = running.length === 0 && notices.length === 0;

  return (
    <PopoverContent
      side="top"
      align="end"
      className="w-80 gap-0 p-0"
      initialFocus={titleRef}
    >
      <div className="flex items-center justify-between border-b border-border py-1.5 pr-1.5 pl-2.5">
        <PopoverTitle
          ref={titleRef}
          tabIndex={-1}
          className="text-xs font-medium outline-none"
        >
          {m['app.notifications.label']()}
        </PopoverTitle>
        <Button
          variant="ghost"
          size="xs"
          disabled={notices.length === 0}
          onClick={() => {
            clearNotices();
            titleRef.current?.focus();
          }}
        >
          {m['app.notifications.clear']()}
        </Button>
      </div>
      {empty ? (
        <p className="p-3 text-muted-foreground">
          {m['app.notifications.empty']()}
        </p>
      ) : (
        <ul className="max-h-96 divide-y divide-border overflow-y-auto">
          {running.map((job) => (
            <JobRow key={job.id} job={job} title={viewOf(job).title} />
          ))}
          {notices.map((notice) => (
            <NoticeRow
              key={notice.id}
              notice={notice}
              onRemoved={() => titleRef.current?.focus()}
            />
          ))}
        </ul>
      )}
    </PopoverContent>
  );
}

function JobRow({ job, title }: { job: Job; title: string }) {
  const progress = job.progress as ProvisionProgress | null;
  const measured = progress !== null && isMeasurable(progress);
  const percent = measured ? Math.round(overallRatio(progress) * 100) : null;
  const stage = progress ? phaseLabel(progress.phase) : null;
  return (
    <li className="flex items-start gap-2 p-2.5">
      <Spinner
        role="presentation"
        aria-hidden
        aria-label={undefined}
        className="mt-px size-4 shrink-0 text-ember"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="truncate font-medium text-foreground">{title}</span>
        {(stage || percent !== null) && (
          <span className="truncate text-muted-foreground tabular-nums">
            {[stage, percent !== null && `${percent}%`]
              .filter(Boolean)
              .join(' · ')}
          </span>
        )}
        <Progress
          value={percent}
          aria-label={title}
          className="[&_[data-slot=progress-indicator]]:bg-ember"
        />
      </div>
      <CancelJobButton job={job} title={title} />
    </li>
  );
}

function NoticeRow({
  notice,
  onRemoved,
}: {
  notice: Notice;
  onRemoved: () => void;
}) {
  const KindIcon = ICONS[notice.kind];
  return (
    <li className="group flex items-start gap-2 p-2.5">
      <KindIcon aria-hidden className="mt-px size-4 shrink-0" />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-baseline gap-2">
          <span
            className={cn(
              'min-w-0 flex-1 break-words text-foreground',
              !notice.read && 'font-medium',
            )}
          >
            {notice.title}
          </span>
          <span className="shrink-0 text-muted-foreground">
            {agoLabel(notice.at / 1000)}
          </span>
        </div>
        {notice.description && (
          <span className="break-words text-muted-foreground">
            {notice.description}
          </span>
        )}
        {notice.action && (
          <Button
            variant="link"
            size="xs"
            className="h-auto self-start p-0"
            onClick={notice.action.onClick}
          >
            {notice.action.label}
          </Button>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon-xs"
        aria-label={m['app.notifications.remove']({ title: notice.title })}
        className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
        onClick={() => {
          removeNotice(notice.id);
          onRemoved();
        }}
      >
        <XIcon />
      </Button>
    </li>
  );
}
