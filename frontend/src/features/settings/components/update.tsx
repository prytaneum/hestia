import { ArrowClockwiseIcon, DownloadSimpleIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import type { UpdateChannel } from '@/api/types/update';
import { Markdown } from '@/components/markdown';
import { OfflineNotice } from '@/components/offline-state';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
} from '@/components/ui/field';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { useConfig } from '@/features/settings/use-config';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { appQueries } from '@/queries/app';
import { useJobMutation } from '@/queries/jobs';
import { useOffline } from '@/queries/net';
import { updateMutations, useUpdateCheck } from '@/queries/update';

const CHANNELS: UpdateChannel[] = ['stable', 'beta'];

const CHANNEL_LABELS: Record<UpdateChannel, () => string> = {
  stable: m['domain.update_channel.stable'],
  beta: m['domain.update_channel.beta'],
};

/** Which feed {@link UpdatePanel} checks — a daemon setting, not a preference. */
export function UpdateChannelField() {
  const { entries, save } = useConfig();

  return (
    <Field>
      <FieldLabel htmlFor="update-channel">
        {m['settings.update.channel_label']()}
      </FieldLabel>
      <Select
        value={entries.update?.channel ?? 'stable'}
        onValueChange={(value) => {
          if (value) save('update.channel', value);
        }}
      >
        <SelectTrigger id="update-channel" className="w-full max-w-md">
          <SelectValue>
            {(value: string) => CHANNEL_LABELS[value as UpdateChannel]()}
          </SelectValue>
        </SelectTrigger>
        <SelectContent align="start">
          <SelectGroup>
            {CHANNELS.map((channel) => (
              <SelectItem key={channel} value={channel}>
                {CHANNEL_LABELS[channel]()}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
      <FieldDescription>{m['settings.update.channel_hint']()}</FieldDescription>
    </Field>
  );
}

/**
 * Self-update. The check runs only when asked — it reaches the network, and an
 * update is not something to nag about before the user looks.
 */
export function UpdatePanel() {
  const app = useQuery(appQueries.info());
  const [asked, setAsked] = useState(false);
  const offline = useOffline();
  const check = useUpdateCheck(asked);
  const download = useJobMutation(updateMutations.download());
  const apply = useMutation(updateMutations.apply());

  const update = check.data?.available;
  const version = app.data?.version ?? '';
  const working = download.isPending || apply.isPending;

  const install = () =>
    download.mutate(undefined, {
      onSuccess: (staged) =>
        apply.mutate(staged.path, {
          onSuccess: (applied) => {
            if (!applied.relaunches) {
              toast.success(m['settings.update.restart_required']());
            }
          },
        }),
    });

  return (
    <>
      <Field orientation="horizontal">
        <FieldContent>
          <FieldLabel className="font-normal">
            {m['settings.update.current']({ version })}
          </FieldLabel>
          {offline && <OfflineNotice />}
          {asked && !check.isFetching && (
            <FieldDescription>
              {check.isError
                ? m['settings.update.check_failed']()
                : !update
                  ? m['settings.update.up_to_date']()
                  : update.applicable
                    ? m['settings.update.available']({
                        version: update.version,
                      })
                    : m['settings.update.manual']({ version: update.version })}
            </FieldDescription>
          )}
        </FieldContent>
        <div className="flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            data-icon="inline-start"
            onClick={() => {
              setAsked(true);
              void check.refetch();
            }}
            disabled={check.isFetching || offline}
          >
            <ArrowClockwiseIcon
              className={cn('size-4', check.isFetching && 'animate-spin')}
            />
            {check.isFetching
              ? m['settings.update.checking']()
              : m['settings.update.check']()}
          </Button>
          {update?.applicable && (
            <Button
              size="sm"
              data-icon="inline-start"
              onClick={install}
              disabled={working}
            >
              <DownloadSimpleIcon className="size-4" />
              {download.isPending
                ? m['settings.update.downloading']()
                : apply.isPending
                  ? m['settings.update.installing']()
                  : m['settings.update.install']()}
            </Button>
          )}
        </div>
      </Field>
      {update?.notes && (
        <Field>
          <FieldLabel className="font-normal">
            {m['settings.update.notes']()}
          </FieldLabel>
          <Markdown>{update.notes}</Markdown>
        </Field>
      )}
    </>
  );
}
