import { XIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useRef } from 'react';

import {
  type ContentKind,
  type ContentProject,
  type ContentVersion,
  content as contentApi,
} from '@/api';
import { contentKindLabel } from '@/components/icons';
import { Badge } from '@/components/ui/badge';
import {
  Combobox,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxList,
} from '@/components/ui/combobox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  projectKey,
  projectRef,
  SourceBadge,
} from '@/features/content/components';
import { kindInfo } from '@/features/shared/content/lib';
import { agoLabel } from '@/lib/format';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { contentQueries, useContentSources } from '@/queries/content';

import { useTarget } from '../hooks';
import type { PickedFile } from '../lib';

export function ReviewStep({
  picked,
  files,
  versionIds,
  onVersion,
  onRemoveProject,
  onRemoveFile,
  onSetFileKind,
  onSetFileTrack,
  worlds,
}: {
  picked: ContentProject[];
  files: PickedFile[];
  versionIds: Record<string, string>;
  onVersion: (ref: string, id: string) => void;
  onRemoveProject: (p: ContentProject) => void;
  onRemoveFile: (path: string) => void;
  onSetFileKind: (path: string, kind: ContentKind) => void;
  onSetFileTrack: (path: string, track: string) => void;
  worlds?: string[];
}) {
  const target = useTarget();

  return (
    <div className="flex flex-col gap-4 p-1">
      <div className="divide-y divide-border border border-border">
        <ReviewRow
          label={m['app.label.target']()}
          value={target?.name ?? '—'}
        />
        {worlds && (
          <ReviewRow
            label={m['app.label.worlds']()}
            value={
              worlds.length ? worlds.join(', ') : m['content.none_selected']()
            }
          />
        )}
      </div>

      <div className="divide-y divide-border border border-border">
        {picked.map((p) => (
          <ReviewItemRow
            key={projectKey(p)}
            project={p}
            versionId={versionIds[projectKey(p)] ?? ''}
            onVersion={(id) => onVersion(projectKey(p), id)}
            onRemove={() => onRemoveProject(p)}
          />
        ))}
        {files.map((f) => (
          <FileReviewRow
            key={f.path}
            file={f}
            onSetKind={(kind) => onSetFileKind(f.path, kind)}
            onSetTrack={(track) => onSetFileTrack(f.path, track)}
            onRemove={() => onRemoveFile(f.path)}
          />
        ))}
      </div>
    </div>
  );
}

function installDir(kind: ContentKind): string {
  return kind === 'data_pack'
    ? m['content.world_datapacks']()
    : `${kindInfo[kind].slug}/`;
}

function FileReviewRow({
  file,
  onSetKind,
  onSetTrack,
  onRemove,
}: {
  file: PickedFile;
  onSetKind: (kind: ContentKind) => void;
  onSetTrack: (track: string) => void;
  onRemove: () => void;
}) {
  const kinds = useTarget()?.accepts ?? [];

  if (!file.valid) {
    return (
      <div className="flex items-center justify-between gap-4 px-3 py-2 text-sm">
        <div className="min-w-0">
          <span className="block truncate">{file.filename}</span>
          <span className="block text-[11px] text-destructive">
            {file.reason}
          </span>
        </div>
        <RemoveButton onClick={onRemove} />
      </div>
    );
  }

  const tracked = file.matches.find((mt) => mt.project.source === file.track);

  return (
    <div className="flex flex-col gap-2 px-3 py-2 text-sm">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <span className="block truncate">
            {tracked?.project.title ?? file.filename}
          </span>
          <span className="block truncate text-[11px] text-muted-foreground">
            {file.kind
              ? m['content.install_to']({ dir: installDir(file.kind) })
              : m['content.choose_type']()}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <KindPicker kinds={kinds} value={file.kind} onChange={onSetKind} />
          <RemoveButton onClick={onRemove} />
        </div>
      </div>
      {file.matches.length > 0 ? (
        <TrackPicker file={file} onChange={onSetTrack} />
      ) : (
        <span className="text-[11px] text-muted-foreground">
          {m['content.track.unrecognized']()}
        </span>
      )}
    </div>
  );
}

function TrackPicker({
  file,
  onChange,
}: {
  file: PickedFile;
  onChange: (track: string) => void;
}) {
  const sources = useContentSources().data ?? [];
  const options = [
    ...file.matches.map(({ project, versionNumber }) => ({
      value: project.source,
      label: `${sources.find((s) => s.id === project.source)?.name ?? project.source} · ${project.title} ${versionNumber}`,
    })),
    { value: contentApi.LOCAL_SOURCE, label: m['content.track.local']() },
  ];
  return (
    <div className="flex items-center gap-2">
      <span className="shrink-0 text-[11px] text-muted-foreground">
        {m['content.track.label']()}
      </span>
      <Select
        items={options}
        value={file.track}
        onValueChange={(value) => onChange(String(value))}
      >
        <SelectTrigger size="sm" className="min-w-0 flex-1">
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
    </div>
  );
}

function KindPicker({
  kinds,
  value,
  onChange,
}: {
  kinds: ContentKind[];
  value: ContentKind | undefined;
  onChange: (kind: ContentKind) => void;
}) {
  return (
    <div className="flex items-center gap-1">
      {kinds.map((k) => (
        <button
          key={k}
          type="button"
          onClick={() => onChange(k)}
          className={cn(
            'h-8 border px-2 text-[11px] outline-none transition-colors focus-visible:ring-1 focus-visible:ring-ring',
            value === k
              ? 'border-ember bg-ember/10 text-foreground'
              : 'border-border text-muted-foreground hover:bg-muted/60',
          )}
        >
          {contentKindLabel[k]()}
        </button>
      ))}
    </div>
  );
}

function RemoveButton({ onClick }: { onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={m['app.action.remove']()}
      className="flex size-8 shrink-0 items-center justify-center border border-border text-muted-foreground outline-none transition-colors hover:border-destructive/40 hover:bg-destructive/10 hover:text-destructive focus-visible:ring-1 focus-visible:ring-ring"
    >
      <XIcon weight="bold" className="size-3.5" />
    </button>
  );
}

function ReviewItemRow({
  project,
  versionId,
  onVersion,
  onRemove,
}: {
  project: ContentProject;
  versionId: string;
  onVersion: (id: string) => void;
  onRemove: () => void;
}) {
  const target = useTarget();
  const versions = useQuery({
    ...contentQueries.versions({
      source: project.source,
      project: projectRef(project),
      loader:
        project.kind === 'mod' ? (target?.flavor ?? undefined) : undefined,
      gameVersion: target?.gameVersion || undefined,
    }),
    enabled: projectRef(project).length > 0,
  });
  const list = versions.data ?? [];
  const resolved = list.find((v) => v.id === versionId) ?? list[0];
  const requiredDeps =
    resolved?.dependencies.filter((d) => d.kind === 'required').length ?? 0;

  return (
    <div className="flex items-center justify-between gap-4 px-3 py-2 text-sm">
      <div className="min-w-0">
        <span className="block truncate">{project.title}</span>
        <span className="block truncate text-[11px] text-muted-foreground">
          {contentKindLabel[project.kind]()}
          {requiredDeps > 0 &&
            ` · ${m['content.dependencies']({ count: requiredDeps })}`}
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <SourceBadge source={project.source} />
        {resolved && !versionId && (
          <Badge variant="secondary" className="shrink-0">
            {m['app.label.latest']()}
          </Badge>
        )}
        {resolved && (
          <VersionCombobox
            versions={list}
            value={resolved}
            onChange={(v) => onVersion(v && v.id !== list[0]?.id ? v.id : '')}
          />
        )}
        <RemoveButton onClick={onRemove} />
      </div>
    </div>
  );
}

function VersionCombobox({
  versions,
  value,
  onChange,
}: {
  versions: ContentVersion[];
  value: ContentVersion;
  onChange: (version: ContentVersion | null) => void;
}) {
  const rootRef = useRef<HTMLDivElement>(null);
  const latestId = versions[0]?.id;
  return (
    <div ref={rootRef} className="contents">
      <Combobox
        items={versions}
        value={value}
        onValueChange={(v: ContentVersion | null) => {
          onChange(v);
          requestAnimationFrame(() =>
            rootRef.current?.querySelector('input')?.blur(),
          );
        }}
        itemToStringLabel={(v: ContentVersion) => v.versionNumber}
        itemToStringValue={(v: ContentVersion) => v.versionNumber}
      >
        <ComboboxInput
          placeholder={m['content.select_version']()}
          className="w-48"
        />
        <ComboboxContent>
          <ComboboxEmpty>{m['content.no_versions']()}</ComboboxEmpty>
          <ComboboxList>
            {(v: ContentVersion) => (
              <ComboboxItem key={v.id} value={v}>
                <div className="flex min-w-0 flex-col">
                  <span className="flex items-center gap-1.5">
                    {v.versionNumber}
                    {v.id === latestId && (
                      <Badge variant="secondary" className="text-[10px]">
                        {m['app.label.latest']()}
                      </Badge>
                    )}
                    {v.channel !== 'release' && (
                      <Badge
                        variant="outline"
                        className="text-[10px] capitalize"
                      >
                        {v.channel}
                      </Badge>
                    )}
                  </span>
                  <span className="truncate font-mono text-[11px] text-muted-foreground">
                    {v.gameVersions.join(', ')} ·{' '}
                    {agoLabel(Date.parse(v.datePublished) / 1000)}
                  </span>
                </div>
              </ComboboxItem>
            )}
          </ComboboxList>
        </ComboboxContent>
      </Combobox>
    </div>
  );
}

function ReviewRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center justify-between gap-4 px-3 py-2 text-sm">
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="truncate text-right text-xs">{value}</span>
    </div>
  );
}
