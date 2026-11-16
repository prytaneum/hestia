import { createFileRoute, redirect } from '@tanstack/react-router';
import type { ContentKind } from '@/api';
import {
  InstanceDetailPage,
  type InstanceTab,
} from '@/features/instances/detail';
import { isContentKind } from '@/features/shared/content/lib';
import { ensureSignedIn } from '@/queries';

const tabs: Record<Exclude<InstanceTab, 'overview'>, true> = {
  content: true,
  worlds: true,
  servers: true,
  logs: true,
  settings: true,
};

export const Route = createFileRoute('/_app/instances/$id')({
  beforeLoad: async ({ context }) => {
    if (!(await ensureSignedIn(context.queryClient))) {
      throw redirect({ to: '/instances' });
    }
  },
  validateSearch: (
    search: Record<string, unknown>,
  ): { tab?: InstanceTab; kind?: ContentKind } => {
    const tab = Object.hasOwn(tabs, search.tab as string)
      ? (search.tab as InstanceTab)
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
    <InstanceDetailPage
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
