import {
  PuzzlePieceIcon,
  TrashIcon,
  WarningCircleIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';

import type { PluginInfo, PluginSource } from '@/api';
import { DetailHero } from '@/components/detail-hero';
import { Empty } from '@/components/empty';
import { ExternalLink } from '@/components/external-link';
import { Section, Stat } from '@/components/page';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { Switch } from '@/components/ui/switch';
import { Notice } from '@/components/warning-notice';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

import { usePluginCommands } from '../commands';
import { PluginSettingsSections } from '../slots';
import { describeGrant } from './approval';
import { GrantList } from './grant';
import { InstallReview, usePluginInstall } from './install';
import { PluginSettings } from './settings';
import {
  attentionReason,
  FullAccessBadge,
  StateBadge,
  usePluginIcon,
} from './state';

const SOURCE_LABEL: Record<PluginSource, () => string> = {
  file: m['domain.plugin.source.file'],
  folder: m['domain.plugin.source.folder'],
  url: m['domain.plugin.source.url'],
  repo: m['domain.plugin.source.repo'],
  dev: m['domain.plugin.source.dev'],
};

/** One installed plugin: its state and actions, settings, commands, access and origin. */
export function PluginDetailPage({ id }: { id: string }) {
  const plugins = useQuery(pluginQueries.list());
  const plugin = plugins.data?.find((listed) => listed.manifest.id === id);

  if (plugins.isPending) {
    return (
      <div className="p-5">
        <Bone className="h-32" />
      </div>
    );
  }
  if (!plugin) {
    return (
      <div className="p-6">
        <Empty icon={WarningCircleIcon}>
          {m['plugins.manage.detail.missing']()}
        </Empty>
      </div>
    );
  }
  return <Detail plugin={plugin} />;
}

function Detail({ plugin }: { plugin: PluginInfo }) {
  const navigate = useNavigate();
  const outdated = useQuery(pluginQueries.outdated());
  const repositories = useQuery(pluginQueries.repositories());
  const enable = useMutation(pluginMutations.enable());
  const disable = useMutation(pluginMutations.disable());
  const remove = useMutation(pluginMutations.remove());
  const install = usePluginInstall();
  const icon = usePluginIcon(plugin);
  const { id, name, version, description, authors, homepage, settings } =
    plugin.manifest;
  const commands = usePluginCommands({ plugin: id });
  const label = name || id;
  const on = plugin.state === 'enabled';
  const reason = attentionReason(plugin);
  const update = outdated.data?.plugins.find((offer) => offer.id === id);
  const store = repositories.data?.find(
    (repository) => repository.url === plugin.repository,
  );

  return (
    <div className="flex h-full flex-col">
      <DetailHero
        parentLabel={m['app.nav.plugins']()}
        parentTo="/plugins"
        icon={PuzzlePieceIcon}
        iconUrl={icon}
        name={label}
        badges={
          <>
            <span className="font-mono text-xs text-muted-foreground">
              {version}
            </span>
            <StateBadge plugin={plugin} />
            <FullAccessBadge plugin={plugin} />
            {plugin.official && (
              <Badge
                variant="secondary"
                title={m['settings.plugins.official_hint']()}
              >
                {m['settings.plugins.official']()}
              </Badge>
            )}
            {plugin.obsolete && (
              <Badge
                variant="secondary"
                title={m['settings.plugins.obsolete_hint']()}
              >
                {m['settings.plugins.obsolete']()}
              </Badge>
            )}
            {plugin.source === 'dev' && (
              <Badge
                variant="secondary"
                title={m['settings.plugins.dev_hint']()}
              >
                {m['settings.plugins.dev']()}
              </Badge>
            )}
          </>
        }
        actions={
          <>
            {update && (
              <Button
                size="sm"
                disabled={install.busy || !update.apiCompatible}
                title={
                  update.apiCompatible
                    ? undefined
                    : m['settings.plugins.update_needs_api']({
                        version: update.available,
                        api: update.requires ?? '',
                      })
                }
                onClick={() => void install.update(update, plugin.url)}
              >
                {m['settings.plugins.update_to']({ version: update.available })}
              </Button>
            )}
            {!update && plugin.source === 'url' && !plugin.signer && (
              <Button
                variant="outline"
                size="sm"
                disabled={install.busy || !plugin.url}
                title={
                  plugin.url
                    ? undefined
                    : m['settings.plugins.check_update_no_link']()
                }
                onClick={() => void install.checkLink(plugin)}
              >
                {m['settings.plugins.check_update']()}
              </Button>
            )}
            {plugin.state !== 'needs_approval' &&
              plugin.state !== 'needs_update' && (
                <Switch
                  aria-label={
                    on ? m['app.action.disable']() : m['app.action.enable']()
                  }
                  checked={on}
                  disabled={enable.isPending || disable.isPending}
                  onCheckedChange={(next) =>
                    (next ? enable : disable).mutate(id)
                  }
                />
              )}
            <ConfirmDialog
              trigger={
                <Button
                  variant="destructive"
                  size="sm"
                  disabled={remove.isPending}
                >
                  <TrashIcon />
                  {m['app.action.remove']()}
                </Button>
              }
              title={m['settings.plugins.remove_title']({ name: label })}
              description={m['settings.plugins.remove_description']()}
              destructive
              confirmLabel={m['app.action.remove']()}
              onConfirm={() =>
                remove.mutate(id, {
                  onSuccess: () => void navigate({ to: '/plugins' }),
                })
              }
            />
          </>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        <div className="max-w-3xl space-y-8">
          {reason && <Notice message={reason} />}
          {install.review && (
            <InstallReview
              inspection={install.review.inspection}
              onContinue={() => {
                if (install.review) void install.approve(install.review);
              }}
              onCancel={install.dismiss}
            />
          )}
          {description && <p className="text-sm">{description}</p>}

          {settings.length > 0 && (
            <Section title={m['settings.plugins.settings.title']()}>
              <PluginSettings plugin={plugin} />
            </Section>
          )}
          <PluginSettingsSections plugin={id} />

          {commands.commands.length > 0 && (
            <Section title={m['plugins.manage.detail.commands']()}>
              <div className="flex flex-wrap gap-2">
                {commands.commands.map((command) => (
                  <Button
                    key={command.command.id}
                    variant="outline"
                    size="sm"
                    title={command.command.description || undefined}
                    onClick={() => commands.run(command)}
                  >
                    {command.command.title || command.command.id}
                  </Button>
                ))}
              </div>
            </Section>
          )}

          <Section title={m['plugins.manage.detail.access']()}>
            <GrantList
              sections={describeGrant(plugin.grant, plugin.unrestricted)}
            />
          </Section>

          <Section title={m['plugins.manage.detail.about']()}>
            <div className="divide-y divide-border border border-border px-3">
              <Stat label={m['plugins.manage.detail.id']()} value={id} />
              {authors.length > 0 && (
                <Stat
                  label={m['plugins.manage.detail.authors']()}
                  value={authors.join(', ')}
                />
              )}
              <Stat
                label={m['plugins.manage.detail.source']()}
                value={SOURCE_LABEL[plugin.source]()}
              />
              {store && (
                <Stat
                  label={m['settings.plugins.browse.repository']()}
                  value={store.name || store.label}
                />
              )}
              {plugin.signer && (
                <Stat
                  label={m['plugins.manage.detail.signer']()}
                  value={plugin.signer}
                />
              )}
              {homepage && (
                <Stat
                  label={m['plugins.manage.detail.homepage']()}
                  value={
                    <ExternalLink href={homepage} className="hover:underline">
                      {homepage}
                    </ExternalLink>
                  }
                />
              )}
            </div>
          </Section>
        </div>
      </div>
      {commands.dialog}
    </div>
  );
}
