import { BroomIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
} from '@/components/ui/field';
import { StatusDot } from '@/components/ui/status-dot';
import { Setting, SettingsSection } from '@/features/settings/components';
import { bytes } from '@/lib/format';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { cacheMutations, cacheQueries } from '@/queries/cache';
import { useDaemon } from '@/queries/daemon';

export function StorageTab() {
  return (
    <>
      <SettingsSection group="storage" legend={m['settings.storage.title']()}>
        <Setting id="cache">
          <CacheField />
        </Setting>
      </SettingsSection>

      <SettingsSection group="daemon" legend={m['settings.daemon.title']()}>
        <Setting id="daemon">
          <DaemonField />
        </Setting>
      </SettingsSection>
    </>
  );
}

function CacheField() {
  const cache = useQuery(cacheQueries.info());
  const clear = useMutation(cacheMutations.clear());

  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel className="gap-2 font-normal">
          {m['settings.download_cache']()}
          <span className="font-mono text-muted-foreground">
            {cache.data ? bytes(cache.data.bytes) : '—'}
          </span>
        </FieldLabel>
        <FieldDescription>{m['settings.storage.hint']()}</FieldDescription>
      </FieldContent>
      <ConfirmDialog
        trigger={
          <Button
            variant="outline"
            size="sm"
            data-icon="inline-start"
            disabled={clear.isPending || !cache.data?.entries}
          >
            <BroomIcon />
            {m['settings.cache.clear']()}
          </Button>
        }
        title={m['settings.cache.clear_title']()}
        description={m['settings.cache.clear_description']()}
        confirmLabel={m['settings.cache.clear']()}
        onConfirm={() =>
          clear.mutate(undefined, {
            onSuccess: (usage) =>
              notify.success(
                m['app.toast.cache_cleared']({ size: bytes(usage.bytes) }),
              ),
          })
        }
      />
    </Field>
  );
}

function DaemonField() {
  const daemon = useDaemon();

  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel className="gap-2 font-normal">
          <StatusDot tone={daemon.connected ? 'on' : 'off'} />
          {daemon.connected
            ? m['app.daemon.connected_label']()
            : m['app.daemon.offline_label']()}
          {daemon.status && (
            <span className="font-mono text-muted-foreground">
              {m['app.daemon.version_uptime']({
                version: daemon.status.version,
                uptime: daemon.uptime ?? '0s',
              })}
            </span>
          )}
        </FieldLabel>
        <FieldDescription>{m['settings.daemon.hint']()}</FieldDescription>
      </FieldContent>
      {daemon.busy ? (
        <Button variant="outline" size="sm" disabled>
          {daemon.restart.isPending
            ? m['app.daemon.restarting']()
            : m['app.daemon.starting']()}
        </Button>
      ) : daemon.connected ? (
        <ConfirmDialog
          trigger={
            <Button variant="outline" size="sm">
              {m['app.daemon.restart']()}
            </Button>
          }
          title={m['app.daemon.restart_title']()}
          description={m['app.daemon.restart_description']()}
          confirmLabel={m['app.action.restart']()}
          onConfirm={() => daemon.restart.mutate()}
        />
      ) : (
        <Button
          variant="outline"
          size="sm"
          onClick={() => daemon.start.mutate()}
        >
          {m['app.daemon.start']()}
        </Button>
      )}
    </Field>
  );
}
