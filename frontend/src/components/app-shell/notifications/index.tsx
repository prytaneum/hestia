import { BellIcon } from '@phosphor-icons/react';

import { Popover, PopoverTrigger } from '@/components/ui/popover';
import { Spinner } from '@/components/ui/spinner';
import {
  setNotificationsOpen,
  useNotificationCentre,
} from '@/lib/notifications';
import { notify } from '@/lib/notify';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useJobs } from '@/queries/jobs';
import { useAnnouncement } from './announcer';
import { NotificationPanel } from './panel';
import { useJobViews } from './views';

/**
 * The status bar's bell: running jobs and every notification of the session,
 * in one panel. A toast is only the moment a notice arrives; once dismissed or
 * expired it is still here. A job a dialog has claimed stays out of the count.
 */
export function Notifications() {
  const jobs = useJobs();
  const { notices, open } = useNotificationCentre();
  const viewOf = useJobViews();

  const running = jobs.filter((job) => job.status === 'running');
  const counted = running.filter((job) => job.background).length;
  const unread = notices.filter((notice) => !notice.read);
  const unreadError = unread.some((notice) => notice.kind === 'error');
  const message = useAnnouncement(
    jobs.filter((job) => job.background),
    viewOf,
  );

  const summary = [
    counted > 0 && m['app.notifications.running_count']({ count: counted }),
    unread.length > 0 &&
      m['app.notifications.unread_count']({ count: unread.length }),
  ]
    .filter(Boolean)
    .join(', ');

  return (
    <div className="ml-auto flex items-center">
      <span role="status" className="sr-only">
        {message}
      </span>
      <Popover
        open={open}
        onOpenChange={(next) => {
          if (next) notify.dismissAll();
          setNotificationsOpen(next);
        }}
      >
        <PopoverTrigger
          aria-label={
            summary
              ? m['app.notifications.summary']({ summary })
              : m['app.notifications.label']()
          }
          title={m['app.notifications.label']()}
          className="flex h-6 items-center gap-1.5 px-1.5 text-muted-foreground outline-none hover:text-foreground focus-visible:text-foreground focus-visible:ring-1 focus-visible:ring-ring aria-expanded:text-foreground"
        >
          {counted > 0 && (
            <>
              <Spinner aria-hidden className="size-3.5 text-ember" />
              <span className="tabular-nums">{counted}</span>
            </>
          )}
          <span className="relative">
            <BellIcon aria-hidden className="size-4" />
            {unread.length > 0 && (
              <span
                aria-hidden
                className={cn(
                  'absolute -top-0.5 -right-0.5 size-1.5',
                  unreadError ? 'bg-destructive' : 'bg-ember',
                )}
              />
            )}
          </span>
        </PopoverTrigger>
        <NotificationPanel
          running={running}
          notices={notices}
          viewOf={viewOf}
        />
      </Popover>
    </div>
  );
}
