import { createFileRoute } from '@tanstack/react-router';
import type { ContentKind } from '@/api';
import { ServerDetailPage, type ServerTab } from '@/features/servers/detail';
import { isContentKind } from '@/features/shared/content/lib';

const tabs: Record<Exclude<ServerTab, 'overview'>, true> = {
  console: true,
  content: true,
  backups: true,
  properties: true,
  settings: true,
};

export const Route = createFileRoute('/_app/servers/$id')({
  validateSearch: (
    search: Record<string, unknown>,
  ): { tab?: ServerTab; kind?: ContentKind } => {
    const tab = Object.hasOwn(tabs, search.tab as string)
      ? (search.tab as ServerTab)
      : undefined;
    return {
      tab,
      kind:
        tab === 'content' && isContentKind(search.kind)
          ? search.kind
          : undefined,
    };
  },
  component: RouteComponent,
});

function RouteComponent() {
  const { id } = Route.useParams();
  const { tab = 'overview', kind } = Route.useSearch();
  const navigate = Route.useNavigate();

  return (
    <ServerDetailPage
      id={id}
      tab={tab}
      onTabChange={(next) =>
        navigate({
          search: next === 'overview' ? {} : { tab: next },
          replace: true,
        })
      }
      contentKind={kind}
      onContentKindChange={(next) =>
        navigate({
          search: { tab: 'content', kind: next },
          replace: true,
        })
      }
    />
  );
}
