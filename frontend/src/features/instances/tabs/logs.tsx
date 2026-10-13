import { FileTextIcon } from '@phosphor-icons/react';

import type { ProcessInfo } from '@/api';
import { Empty } from '@/components/empty';
import { LogView } from '@/components/log-view';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { TabToolbar } from '@/features/shared/entry/detail';
import { sessionSeq } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { useInstanceLogs } from '@/queries/instance';

/** The sentinel for "no session filter" — a Select item needs a value. */
const ALL = 'all';

/**
 * The instance's captured output, followed continuously: the subject is the
 * instance, so a session ending leaves the log in place and the next launch
 * streams into it. The picker narrows the stream to one session.
 */
export function InstanceLogsTab({
  id,
  name,
  sessions,
  session,
  onSessionChange,
}: {
  id: string;
  name: string;
  /** The live sessions, oldest first; the picker appears from two up. */
  sessions: ProcessInfo[];
  /** The session shown; null follows every session of the instance. */
  session: string | null;
  onSessionChange: (session: string | null) => void;
}) {
  // A filter on a session that has since exited is dropped, not left showing
  // an empty view for a name nothing matches any more.
  const selected =
    session && sessions.some((entry) => entry.id === session) ? session : null;
  const logs = useInstanceLogs(id, {
    follow: true,
    tail: 500,
    session: selected ?? undefined,
  });

  const options = [
    { value: ALL, label: m['instance.logs.all_sessions']() },
    ...sessions.map((entry) => ({
      value: entry.id,
      label: m['entry.session.name']({ seq: sessionSeq(entry.id) }),
    })),
  ];

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {sessions.length > 1 && (
        <TabToolbar className="justify-end">
          <Select
            items={options}
            value={selected ?? ALL}
            onValueChange={(value) =>
              onSessionChange(value === ALL ? null : String(value))
            }
          >
            <SelectTrigger className="w-44" size="sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {options.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {option.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </TabToolbar>
      )}

      {logs.lines.length === 0 ? (
        <Empty className="h-full" icon={FileTextIcon}>
          {m['instance.logs_empty']()}
        </Empty>
      ) : (
        <LogView
          aria-label={name}
          rows={logs.lines.map((entry) => ({ text: entry.line }))}
        />
      )}
    </div>
  );
}
