import { WarningCircleIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';

import { Empty } from '@/components/empty';
import { Page } from '@/components/page';
import { m } from '@/paraglide/messages.js';
import { pluginQueries } from '@/queries/plugin';

import { PluginView } from './view';

/** A nav-slot page, full width under the app's usual page header. */
export function PluginPage({ plugin, page }: { plugin: string; page: string }) {
  const pages = useQuery(pluginQueries.pages());
  const found = pages.data?.find(
    (contribution) =>
      contribution.plugin === plugin && contribution.page.id === page,
  );

  if (pages.data && !found) {
    return (
      <Page title={page}>
        <Empty icon={WarningCircleIcon}>{m['plugins.missing']()}</Empty>
      </Page>
    );
  }
  return (
    <Page title={found?.page.title ?? page} loading={pages.isPending}>
      <PluginView plugin={plugin} page={page} />
    </Page>
  );
}
