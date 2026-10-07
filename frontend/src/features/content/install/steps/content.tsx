import { UploadSimpleIcon } from '@phosphor-icons/react';
import { type UseQueryResult, useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import {
  type ContentKind,
  type ContentProject,
  content as contentApi,
  dialog,
  errorMessage,
  type ResolvedUrl,
} from '@/api';
import { FilterMenu } from '@/components/filter-menu';
import { contentIcon, contentKindLabel } from '@/components/icons';
import { OfflineState } from '@/components/offline-state';
import { PickerPanel } from '@/components/picker-panel';
import {
  projectKey,
  sourceGroup,
  useSourceOptions,
} from '@/features/content/components';
import {
  KindChips,
  PickRow,
  PickRowSkeleton,
} from '@/features/shared/content/components';
import { useFileDrop } from '@/hooks/use-file-drop';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { contentQueries, isContentUrl } from '@/queries/content';
import { instanceQueries } from '@/queries/instance';
import { useOffline } from '@/queries/net';

import { FilterBar } from '../filter-bar';
import {
  fileName,
  type PickedFile,
  type Target,
  useInstalledRefs,
} from '../lib';

export function ContentStep({
  target,
  kind,
  onKindChange,
  source,
  onSourceChange,
  picked,
  onToggle,
  onAddFiles,
}: {
  target: Target;
  kind: ContentKind | null;
  onKindChange: (kind: ContentKind | null) => void;
  source: string;
  onSourceChange: (source: string) => void;
  picked: ContentProject[];
  onToggle: (p: ContentProject, versionId?: string) => void;
  onAddFiles: (files: PickedFile[]) => void;
}) {
  const [search, setSearch] = useState('');
  const kinds = target.accepts;
  // Datapacks land inside a world; an instance with none can take none.
  const worlds = useQuery({
    ...instanceQueries.worlds(target.id),
    enabled: target.type === 'instance',
  });
  const noWorlds = target.type === 'instance' && worlds.data?.length === 0;
  const datapackBlocked = (k: ContentKind) => k === 'data_pack' && noWorlds;
  const activeKind = kind ?? kinds[0];
  const pickedKeys = new Set(picked.map(projectKey));
  const installedRefs = useInstalledRefs(target, activeKind);
  const sources = useSourceOptions(activeKind, source);

  const url = isContentUrl(search) ? search.trim() : '';
  const link = useQuery(contentQueries.url(url));

  const results = useQuery({
    ...contentQueries.search({
      kind: activeKind,
      query: search.trim(),
      source: sources.active,
      loader: activeKind === 'mod' ? target.flavor : undefined,
      gameVersion: target.gameVersion || undefined,
      limit: 30,
    }),
    enabled: !url,
  });
  const hits = results.data?.hits ?? [];
  const offline = useOffline();

  return (
    <PickerPanel
      header={
        <>
          <FilterBar
            search={search}
            onSearch={setSearch}
            placeholder={m['app.search.content_or_link']()}
            trailing={
              <FilterMenu
                groups={[
                  sourceGroup(sources.list, sources.active, onSourceChange),
                ]}
              />
            }
          />
          {kinds.length > 1 && (
            <KindChips
              kinds={kinds}
              kind={activeKind}
              onKindChange={(next) => next && onKindChange(next)}
              disabled={datapackBlocked}
              all={false}
              className="mb-3"
            />
          )}

          <FileImportButton onFiles={onAddFiles} />
        </>
      }
    >
      {offline ? (
        <OfflineState />
      ) : datapackBlocked(activeKind) ? (
        <p className="px-1 py-8 text-center text-xs text-muted-foreground">
          {m['content.no_worlds_datapack']()}
        </p>
      ) : url ? (
        <LinkResult
          query={link}
          accepts={kinds}
          installed={installedRefs}
          picked={pickedKeys}
          onToggle={onToggle}
        />
      ) : results.isPending ? (
        <PickRowSkeleton count={5} />
      ) : hits.length === 0 ? (
        <p className="px-1 py-8 text-center text-xs text-muted-foreground">
          {m['content.browse.nothing_matches']()}
        </p>
      ) : (
        <div className="grid gap-2 p-0.5">
          {hits.map((p) => {
            const installed = installedRefs.has(`${p.source}:${p.id}`);
            return (
              <PickRow
                key={projectKey(p)}
                icon={contentIcon(p.kind)}
                imageUrl={p.iconUrl}
                title={p.title}
                subtitle={`${contentKindLabel[p.kind]()} · ${m['content.browse.by_author']({ name: p.author })}`}
                badge={installed ? m['content.installed']() : undefined}
                disabled={installed}
                selected={pickedKeys.has(projectKey(p))}
                onSelect={() => onToggle(p)}
              />
            );
          })}
        </div>
      )}
    </PickerPanel>
  );
}

/** A resolved link as one row; a kind the target refuses is shown, not hidden. */
function LinkResult({
  query,
  accepts,
  installed,
  picked,
  onToggle,
}: {
  query: UseQueryResult<ResolvedUrl>;
  accepts: ContentKind[];
  installed: Set<string>;
  picked: Set<string>;
  onToggle: (p: ContentProject, versionId?: string) => void;
}) {
  if (query.isPending) {
    return <PickRowSkeleton count={1} />;
  }
  if (query.error || !query.data) {
    return (
      <p className="px-1 py-8 text-center text-xs text-destructive">
        {errorMessage(query.error)}
      </p>
    );
  }

  const { project, versionId } = query.data;
  const taken = accepts.includes(project.kind);
  const already = installed.has(`${project.source}:${project.id}`);
  return (
    <div className="grid gap-2 p-0.5">
      <PickRow
        icon={contentIcon(project.kind)}
        imageUrl={project.iconUrl}
        title={project.title}
        subtitle={`${contentKindLabel[project.kind]()} · ${m['content.browse.by_author']({ name: project.author })}`}
        badge={
          taken
            ? already
              ? m['content.installed']()
              : versionId
                ? m['content.pinned_version']()
                : undefined
            : m['content.kind_not_taken']()
        }
        disabled={already || !taken}
        selected={picked.has(projectKey(project))}
        onSelect={() => onToggle(project, versionId)}
      />
    </div>
  );
}

function FileImportButton({
  onFiles,
}: {
  onFiles: (files: PickedFile[]) => void;
}) {
  const stage = async (paths: string[]) => {
    if (paths.length === 0) return;
    const files = await Promise.all(
      paths.map(async (path): Promise<PickedFile> => {
        const r = await contentApi.inspect(path);
        return {
          path,
          filename: r.filename || fileName(path),
          kind: r.kind,
          detected: r.kind,
          valid: r.valid,
          reason: r.reason,
          project: r.project,
          versionNumber: r.versionNumber,
        };
      }),
    );
    onFiles(files);
  };
  const over = useFileDrop(dialog.CONTENT_EXTENSIONS, stage);

  return (
    <button
      type="button"
      onClick={async () => stage(await dialog.pickContentFiles())}
      className={cn(
        'mb-2 flex w-full items-center gap-3 border border-dashed border-border p-3 text-left outline-none transition-colors hover:bg-muted/60 focus-visible:ring-1 focus-visible:ring-ring',
        over && 'border-primary bg-muted/60',
      )}
    >
      <UploadSimpleIcon className="size-4 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm">
          {m['content.import_file']()}
        </span>
        <span className="block truncate text-[11px] text-muted-foreground">
          {m['content.import_file_hint']()}
        </span>
      </span>
    </button>
  );
}
