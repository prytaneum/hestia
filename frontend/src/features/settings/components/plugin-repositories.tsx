import { ArrowClockwiseIcon, TrashIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useCallback, useState } from 'react';

import { errorMessage, type RepositoryAdded, type RepositoryInfo } from '@/api';
import { ExternalLink } from '@/components/external-link';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Field, FieldDescription } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { useRepositoryLink } from '@/features/settings/hooks';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

/**
 * The repositories the user trusts, as stores. Adding one fetches its index and
 * asks first, naming the key id being trusted; nothing is saved until the user
 * agrees.
 */
export function RepositoryList() {
  const repositories = useQuery(pluginQueries.repositories());
  const add = useMutation(pluginMutations.addRepository());
  const refresh = useMutation(pluginMutations.refreshRepositories());
  const [address, setAddress] = useState('');
  const [found, setFound] = useState<RepositoryAdded | null>(null);

  const { mutateAsync: lookUpRepository } = add;
  const lookUp = useCallback(
    async (url: string) => {
      try {
        const result = await lookUpRepository({ url, confirm: false });
        if (result.configured)
          notify.info(m['settings.plugins.repos.already']());
        else setFound(result);
      } catch (error) {
        notify.error(errorMessage(error));
      }
    },
    [lookUpRepository],
  );
  useRepositoryLink(lookUp);

  const trust = async () => {
    if (!found) return;
    try {
      const saved = await add.mutateAsync({
        url: found.repository.url,
        confirm: true,
      });
      notify.success(
        m['settings.plugins.repos.added']({ name: saved.repository.name }),
      );
      setFound(null);
      setAddress('');
    } catch (error) {
      notify.error(errorMessage(error));
    }
  };

  const refreshAll = async () => {
    try {
      const result = await refresh.mutateAsync(undefined);
      notify.warnings(result.warnings, { id: 'plugin-repos-refresh' });
    } catch (error) {
      notify.error(errorMessage(error));
    }
  };

  if (repositories.isPending) return <Bone className="h-20" />;
  const list = repositories.data ?? [];

  return (
    <Field>
      <div className="flex items-center justify-between gap-3">
        <FieldDescription>
          {m['settings.plugins.repos.hint']()}
        </FieldDescription>
        <Button
          variant="outline"
          size="sm"
          disabled={refresh.isPending || list.length === 0}
          onClick={() => void refreshAll()}
        >
          <ArrowClockwiseIcon />
          {m['settings.plugins.repos.refresh']()}
        </Button>
      </div>

      <form
        className="flex gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          if (address.trim()) void lookUp(address.trim());
        }}
      >
        <Input
          value={address}
          onChange={(event) => setAddress(event.target.value)}
          placeholder={m['settings.plugins.repos.add_placeholder']()}
          aria-label={m['settings.plugins.repos.title']()}
        />
        <Button
          type="submit"
          size="sm"
          disabled={add.isPending || !address.trim()}
        >
          {m['settings.plugins.repos.add']()}
        </Button>
      </form>

      <ConfirmDialog
        open={found !== null}
        onOpenChange={(open) => {
          if (!open) setFound(null);
        }}
        title={m['settings.plugins.repos.trust_title']({
          name: found?.repository.name ?? '',
        })}
        description={
          found && (
            <>
              {m['settings.plugins.repos.trust_description']({
                key: found.repository.keyId,
                name: found.repository.name,
              })}
              <span className="mt-2 block font-mono text-[11px]">
                {[
                  found.repository.label,
                  m['settings.plugins.repos.count']({
                    count: found.repository.plugins,
                  }),
                  found.repository.website,
                ]
                  .filter(Boolean)
                  .join(' · ')}
              </span>
            </>
          )
        }
        confirmLabel={m['settings.plugins.repos.trust']()}
        onConfirm={() => void trust()}
      />

      <div className="divide-y divide-border border border-border">
        {list.length === 0 ? (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {m['settings.plugins.repos.empty']()}
          </p>
        ) : (
          list.map((repository) => (
            <RepositoryRow key={repository.url} repository={repository} />
          ))
        )}
      </div>
    </Field>
  );
}

function RepositoryRow({ repository }: { repository: RepositoryInfo }) {
  const remove = useMutation(pluginMutations.removeRepository());
  const name = repository.name || repository.url;

  return (
    <div className="flex items-center gap-3 px-3 py-2">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2 text-sm">
          <span className="truncate">{name}</span>
          <Badge variant="outline">{repository.label}</Badge>
        </div>
        <div className="truncate font-mono text-[11px] text-muted-foreground">
          {[
            m['settings.plugins.key_id']({ key: repository.keyId }),
            m['settings.plugins.repos.count']({ count: repository.plugins }),
          ].join(' · ')}
        </div>
        {repository.website && (
          <ExternalLink
            href={repository.website}
            className="block truncate text-[11px] text-muted-foreground hover:underline"
          >
            {repository.website}
          </ExternalLink>
        )}
      </div>
      {!repository.official && (
        <ConfirmDialog
          trigger={
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={m['app.action.remove']()}
              disabled={remove.isPending}
            >
              <TrashIcon className="size-4" />
            </Button>
          }
          title={m['settings.plugins.repos.remove_title']({ name })}
          description={m['settings.plugins.repos.remove_description']()}
          destructive
          confirmLabel={m['app.action.remove']()}
          onConfirm={() => remove.mutate(repository.url)}
        />
      )}
    </div>
  );
}
