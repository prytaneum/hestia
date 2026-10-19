import { type ReactNode, useEffect, useRef } from 'react';

import { Button } from '@/components/ui/button';
import { PopoverContent, PopoverTitle } from '@/components/ui/popover';
import { m } from '@/paraglide/messages.js';
import { clearSettledJobs, type Job } from '@/queries/jobs';
import { ActivityRow } from './row';
import type { JobView } from './views';

const PANEL = '[data-activity-panel]';

/** The pointed-at row's first action, else the first row's, else the heading. */
function focusTarget(
  focus: string | null,
  heading: HTMLElement | null,
): HTMLElement | null {
  const row = focus ? `${PANEL} [data-job-id="${focus}"] button` : null;
  return (
    (row && document.querySelector<HTMLElement>(row)) ||
    document.querySelector<HTMLElement>(`${PANEL} [data-job-id] button`) ||
    heading
  );
}

export function ActivityPanel({
  jobs,
  viewOf,
  focus,
}: {
  jobs: Job[];
  viewOf: (job: Job) => JobView;
  focus: string | null;
}) {
  const titleRef = useRef<HTMLHeadingElement>(null);
  const running = jobs.filter((job) => job.status === 'running');
  const recent = jobs
    .filter((job) => job.status !== 'running')
    .sort((a, b) => (b.settledAt ?? 0) - (a.settledAt ?? 0));

  const target = () => focusTarget(focus, titleRef.current);

  // A toast can point at a row while the panel is already open.
  useEffect(() => {
    if (!focus) return;
    const frame = requestAnimationFrame(() =>
      focusTarget(focus, titleRef.current)?.focus(),
    );
    return () => cancelAnimationFrame(frame);
  }, [focus]);

  const refocus = () => requestAnimationFrame(() => target()?.focus());

  return (
    <PopoverContent
      side="top"
      align="end"
      className="w-80 gap-0 p-0"
      initialFocus={target}
      data-activity-panel=""
    >
      <PopoverTitle
        ref={titleRef}
        tabIndex={-1}
        className="border-b border-border p-2.5 text-xs font-medium outline-none"
      >
        {m['app.jobs.panel.label']()}
      </PopoverTitle>
      <div className="max-h-80 overflow-y-auto">
        {running.length === 0 && recent.length === 0 && (
          <p className="p-3 text-muted-foreground">
            {m['app.jobs.panel.empty']()}
          </p>
        )}
        {running.length > 0 && (
          <Section label={m['app.jobs.panel.running']()}>
            {running.map((job) => (
              <ActivityRow
                key={job.id}
                job={job}
                view={viewOf(job)}
                onRemoved={refocus}
              />
            ))}
          </Section>
        )}
        {recent.length > 0 && (
          <Section label={m['app.jobs.panel.recent']()}>
            {recent.map((job) => (
              <ActivityRow
                key={job.id}
                job={job}
                view={viewOf(job)}
                onRemoved={refocus}
              />
            ))}
          </Section>
        )}
      </div>
      <div className="flex justify-end border-t border-border p-1.5">
        <Button
          variant="ghost"
          size="xs"
          disabled={recent.length === 0}
          onClick={() => {
            clearSettledJobs();
            refocus();
          }}
        >
          {m['app.jobs.panel.clear']()}
        </Button>
      </div>
    </PopoverContent>
  );
}

function Section({ label, children }: { label: string; children: ReactNode }) {
  return (
    <section className="border-b border-border last:border-b-0">
      <h3 className="px-2.5 pt-2 text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
        {label}
      </h3>
      <ul className="divide-y divide-border p-1">{children}</ul>
    </section>
  );
}
