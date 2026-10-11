import { FileArrowUpIcon, type Icon } from '@phosphor-icons/react';
import type { ReactNode } from 'react';

import { useDropzone } from '@/hooks/use-dropzone';
import { cn } from '@/lib/utils';

/**
 * The one "choose a file" control: click it for the native picker, or drop a
 * file on it. Both end in `onFiles`, so a caller never opens a second surface
 * just to ask for the file.
 */
export function FileDropButton({
  icon: RowIcon = FileArrowUpIcon,
  title,
  hint,
  accept,
  multiple = false,
  pick,
  onFiles,
  className,
}: {
  icon?: Icon;
  title: ReactNode;
  hint?: ReactNode;
  /** Accepted file extensions, without the dot. */
  accept: readonly string[];
  multiple?: boolean;
  /** Open the native picker; resolves empty when it is dismissed. */
  pick: () => Promise<string[]>;
  onFiles: (paths: string[]) => void;
  className?: string;
}) {
  const take = (paths: string[]) => {
    if (paths.length > 0) onFiles(paths);
  };
  const { getRootProps, isDragActive } = useDropzone<HTMLButtonElement>({
    accept,
    multiple,
    onDrop: take,
  });

  return (
    <button
      {...getRootProps()}
      type="button"
      onClick={async () => take(await pick())}
      className={cn(
        'flex w-full items-center gap-3 border border-border border-dashed px-3 py-2.5 text-left outline-none transition-colors hover:bg-accent/50 focus-visible:ring-1 focus-visible:ring-ring',
        isDragActive && 'border-primary bg-primary/5',
        className,
      )}
    >
      <RowIcon className="size-4 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-medium text-sm">{title}</span>
        {hint && (
          <span className="block truncate text-muted-foreground text-xs">
            {hint}
          </span>
        )}
      </span>
    </button>
  );
}
