import type { Icon } from '@phosphor-icons/react';
import type { ReactNode } from 'react';

import {
  Dialog,
  DialogBody,
  DialogContent,
  type DialogContentVariants,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';

/**
 * The parent of every dialog in the app. It owns the chrome — the box and its
 * size limits, the header, the close button — so a dialog is only its content:
 * a `DialogBody` (the part that scrolls) and, usually, a `DialogFooter`.
 *
 * Omit `title` for a dialog that draws its own heading inside the content.
 */
export function AppDialog({
  open,
  onOpenChange,
  size = 'md',
  height,
  icon: TitleIcon,
  title,
  description,
  showCloseButton,
  className,
  children,
}: {
  open: boolean;
  onOpenChange?: (open: boolean) => void;
  icon?: Icon;
  title?: ReactNode;
  description?: ReactNode;
  showCloseButton?: boolean;
  className?: string;
  children: ReactNode;
} & DialogContentVariants) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        size={size}
        height={height}
        showCloseButton={showCloseButton}
        className={className}
      >
        {title && (
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              {TitleIcon && (
                <TitleIcon className="size-4.5 shrink-0 text-muted-foreground" />
              )}
              {title}
            </DialogTitle>
            {description && (
              <DialogDescription>{description}</DialogDescription>
            )}
          </DialogHeader>
        )}
        {children}
      </DialogContent>
    </Dialog>
  );
}

export { DialogBody, DialogFooter };
