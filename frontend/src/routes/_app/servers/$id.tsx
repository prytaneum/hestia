import { createFileRoute } from '@tanstack/react-router';
import type { ContentKind } from '@/api';
import { ServerDetailPage, type ServerTab } from '@/features/servers/detail';
import { isContentKind } from '@/features/shared/content/lib';

const tabs: Record<Exclude<ServerTab, 'overview'>, true> = {
  console: true,
  players: true,
  content: true,
  files: true,
  backups: true,
  settings: true,
};

interface Search {
  tab?: ServerTab;
  kind?: ContentKind;
  /** The directory the files tab is showing. */
  path?: string;
  /** The file the files tab has open. */
  file?: string;
}

const text = (value: unknown): string | undefined =>
  typeof value === 'string' && value !== '' ? value : undefined;

export const Route = createFileRoute('/_app/servers/$id')({
  validateSearch: (search: Record<string, unknown>): Search => {
    const tab = Object.hasOwn(tabs, search.tab as string)
      ? (search.tab as ServerTab)
      : undefined;
    return {
      tab,
      kind:
        tab === 'content' && isContentKind(search.kind)
          ? search.kind
          : undefined,
      path: tab === 'files' ? text(search.path) : undefined,
      file: tab === 'files' ? text(search.file) : undefined,
    };
  },
  component: RouteComponent,
});

function RouteComponent() {
  const { id } = Route.useParams();
  const { tab = 'overview', kind, path = '', file } = Route.useSearch();
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
      filesPath={path}
      openFile={file}
      // Pushed, not replaced: Back walks out of a folder or an open file.
      onFilesNavigate={(next) =>
        navigate({ search: { tab: 'files', path: next || undefined } })
      }
      onOpenFile={(next) =>
        navigate({
          search: { tab: 'files', path: path || undefined, file: next },
        })
      }
    />
  );
}
