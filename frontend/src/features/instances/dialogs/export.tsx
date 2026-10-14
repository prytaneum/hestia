import { CaretRightIcon } from '@phosphor-icons/react';
import { useQuery } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { toast } from 'sonner';

import type { ArchiveEntry, ExportFormat } from '@/api';
import { dialog, errorMessage, system } from '@/api';
import { AppDialog, DialogFooter } from '@/components/dialog';
import { Bone } from '@/components/skeleton';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { bytes } from '@/lib/format';
import { toastWarnings } from '@/lib/warnings';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { transferMutations, transferQueries } from '@/queries/transfer';

import { buildTree, excludedRoots, selectedBytes, type TreeNode } from '../lib';

const FORMATS: ExportFormat[] = ['hestia', 'mrpack'];

const formatOptions = () =>
  FORMATS.map((value) => ({
    value,
    label: m[`domain.export_format.${value}`](),
  }));

/**
 * Export one instance to an archive. The tree is what the daemon says the
 * archive *would* carry, so unchecking a node is a decision made against the
 * real thing rather than a guess — and the exclusions it produces are the
 * topmost unchecked paths, since everything under one is implied.
 *
 * The job runs in the background: an instance with a few gigabytes of worlds
 * takes a while, and holding a modal open for it would be the only thing in the
 * app that traps you on a page.
 */
export function ExportInstanceDialog({
  id,
  name,
  open,
  onOpenChange,
}: {
  id: string;
  name: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [format, setFormat] = useState<ExportFormat>('hestia');
  const formats = formatOptions();
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['data']));

  const contents = useQuery({ ...transferQueries.contents(id), enabled: open });
  const entries = useMemo<ArchiveEntry[]>(
    () => contents.data ?? [],
    [contents.data],
  );
  const tree = useMemo(() => buildTree(entries), [entries]);
  const total = useMemo(
    () =>
      entries
        .filter((e) => !e.path.includes('/'))
        .reduce((sum, e) => sum + e.sizeBytes, 0),
    [entries],
  );
  const selected = useMemo(
    () => selectedBytes(tree, excluded),
    [tree, excluded],
  );

  const run = useJobMutation(transferMutations.export(id, name));

  const toggle = (path: string, checked: boolean) => {
    setExcluded((current) => {
      const next = new Set(current);
      // Re-including a node means nothing while an ancestor is out, so an
      // ancestor's exclusion is lifted with it.
      for (const path_ of next) {
        if (path === path_ || path.startsWith(`${path_}/`)) next.delete(path_);
      }
      for (const path_ of [...next]) {
        if (path_.startsWith(`${path}/`)) next.delete(path_);
      }
      if (!checked) next.add(path);
      return next;
    });
  };

  const start = async () => {
    const extension = format === 'hestia' ? 'hestia' : 'mrpack';
    const destination = await dialog.pickExportPath(
      `${name}.${extension}`,
      extension,
    );
    if (!destination) return;
    onOpenChange(false);
    run.mutate(
      { format, destination, exclude: excludedRoots(excluded) },
      {
        onSuccess: (done) => {
          toast.success(m['instance.export.done']({ name }), {
            description: m['instance.export.summary']({
              files: done.files,
              size: bytes(done.sizeBytes),
            }),
            action: {
              label: m['app.action.open_folder'](),
              onClick: () => void system.openPath(parentOf(done.path)),
            },
          });
          toastWarnings(done.warnings);
        },
        onError: (error) => toast.error(errorMessage(error)),
      },
    );
  };

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      size="lg"
      title={m['instance.export.title']({ name })}
      description={
        format === 'hestia'
          ? m['instance.export.hint_hestia']()
          : m['instance.export.hint_mrpack']()
      }
    >
      <div className="flex items-center gap-2">
        <span className="text-muted-foreground text-xs">
          {m['instance.export.format']()}
        </span>
        <Select
          items={formats}
          value={format}
          onValueChange={(value) => setFormat(value as ExportFormat)}
        >
          <SelectTrigger className="w-44" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {formats.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      <div className="max-h-72 min-h-24 overflow-y-auto border">
        {contents.isPending ? (
          <div className="flex flex-col gap-2 p-3">
            <Bone className="h-4 w-2/3" />
            <Bone className="h-4 w-1/2" />
            <Bone className="h-4 w-3/5" />
          </div>
        ) : tree.length === 0 ? (
          <p className="p-4 text-center text-muted-foreground text-sm">
            {m['instance.export.empty']()}
          </p>
        ) : (
          <ul className="p-1">
            {tree.map((node) => (
              <Row
                key={node.path}
                node={node}
                depth={0}
                excluded={excluded}
                expanded={expanded}
                onToggle={toggle}
                onExpand={(path) =>
                  setExpanded((current) => {
                    const next = new Set(current);
                    if (!next.delete(path)) next.add(path);
                    return next;
                  })
                }
              />
            ))}
          </ul>
        )}
      </div>

      <DialogFooter className="sm:justify-between">
        <span className="text-muted-foreground text-xs">
          {m['instance.export.selected']({
            selected: bytes(selected),
            total: bytes(total),
          })}
        </span>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {m['app.action.cancel']()}
          </Button>
          <Button onClick={start} disabled={contents.isPending}>
            {m['app.action.export']()}
          </Button>
        </div>
      </DialogFooter>
    </AppDialog>
  );
}

/** One node and, when it is an expanded directory, its children. */
function Row({
  node,
  depth,
  excluded,
  expanded,
  onToggle,
  onExpand,
}: {
  node: TreeNode;
  depth: number;
  excluded: Set<string>;
  expanded: Set<string>;
  onToggle: (path: string, checked: boolean) => void;
  onExpand: (path: string) => void;
}) {
  // A node is out when it or any ancestor is: excluding `data/saves` takes
  // every world with it, and the boxes have to say so.
  const out = [...excluded].some(
    (path) => node.path === path || node.path.startsWith(`${path}/`),
  );
  const isOpen = expanded.has(node.path);
  const hasChildren = node.children.length > 0;

  return (
    <li>
      <div
        className="flex items-center gap-2 px-2 py-1 hover:bg-accent/50"
        style={{ paddingLeft: `${depth * 1.25 + 0.5}rem` }}
      >
        {hasChildren ? (
          <button
            type="button"
            aria-label={node.name}
            onClick={() => onExpand(node.path)}
            className="grid size-4 place-items-center text-muted-foreground outline-none hover:text-foreground"
          >
            <CaretRightIcon
              className={`size-3 transition-transform ${isOpen ? 'rotate-90' : ''}`}
            />
          </button>
        ) : (
          <span className="size-4" />
        )}
        <Checkbox
          checked={!out}
          onCheckedChange={(checked) => onToggle(node.path, checked === true)}
        />
        <span className="min-w-0 flex-1 truncate text-xs">
          {node.name}
          {node.directory ? '/' : ''}
        </span>
        <span className="text-muted-foreground text-xs tabular-nums">
          {node.referenced
            ? m['instance.export.referenced']()
            : bytes(node.sizeBytes)}
        </span>
      </div>
      {isOpen && hasChildren && (
        <ul>
          {node.children.map((child) => (
            <Row
              key={child.path}
              node={child}
              depth={depth + 1}
              excluded={excluded}
              expanded={expanded}
              onToggle={onToggle}
              onExpand={onExpand}
            />
          ))}
        </ul>
      )}
    </li>
  );
}

function parentOf(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut > 0 ? path.slice(0, cut) : path;
}
