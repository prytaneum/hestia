import { CheckIcon, XIcon } from '@phosphor-icons/react';
import { motion } from 'motion/react';
import { Children } from 'react';

import { CapeFront } from '@/features/skins/components';
import { listContainer, listItem } from '@/lib/motion';
import { cn } from '@/lib/utils';

export function CapeGrid({ children }: { children: React.ReactNode }) {
  return (
    <motion.div
      initial="hidden"
      animate="show"
      variants={listContainer(Children.count(children))}
      className="grid grid-cols-[repeat(auto-fill,minmax(5.5rem,1fr))] gap-1.5"
    >
      {children}
    </motion.div>
  );
}

/**
 * One cape choice: a Mojang-owned cape, or — with no `texture` — the
 * "no cape" card. Clicking applies immediately; capes are account-level,
 * never bound to a skin.
 */
export function CapeCard({
  label,
  texture,
  equipped,
  onEquip,
}: {
  label: string;
  texture?: string;
  equipped: boolean;
  onEquip: () => void;
}) {
  return (
    <motion.button
      variants={listItem}
      type="button"
      disabled={equipped}
      onClick={onEquip}
      aria-pressed={equipped}
      className={cn(
        'relative flex flex-col items-center gap-1.5 px-1 pt-2.5 pb-1.5 ring-1 transition-colors outline-none focus-visible:ring-ring disabled:opacity-70',
        equipped
          ? 'bg-muted ring-ember'
          : 'ring-border hover:bg-muted/60 hover:ring-foreground/20',
      )}
    >
      {equipped && (
        <CheckIcon
          weight="bold"
          className="absolute top-1.5 right-1.5 size-3.5 text-ember"
        />
      )}
      <span className="grid h-12 place-items-center">
        {texture ? (
          <CapeFront texture={texture} className="h-12" />
        ) : (
          <XIcon className="size-5 text-muted-foreground" />
        )}
      </span>
      <span className="w-full truncate text-center text-[10px] text-muted-foreground">
        {label}
      </span>
    </motion.button>
  );
}
