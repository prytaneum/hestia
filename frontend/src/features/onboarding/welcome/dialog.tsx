import { AnimatePresence, motion } from 'motion/react';
import { useState } from 'react';

import { Logo } from '@/components/app-shell/logo';
import { AppDialog } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { duration, EASE_OUT } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { useDaemon } from '@/queries/daemon';

import { useOnboarding } from '../state';
import { useTour } from '../tour';
import { slides } from './slides';

export function WelcomeDialog() {
  const onboarding = useOnboarding();
  const accounts = useAccounts();
  const daemon = useDaemon();
  const { start } = useTour();
  const [index, setIndex] = useState(0);

  const open =
    onboarding.ready &&
    !onboarding.welcomed &&
    accounts.ready &&
    daemon.connected;

  const slide = slides[index];
  const last = index === slides.length - 1;

  const finish = (tour: boolean) => {
    onboarding.update({ welcomed: true });
    if (tour) start('shell');
  };

  return (
    <AppDialog open={open} showCloseButton={false} className="gap-0 p-0">
      <div className="flex flex-col items-center gap-5 px-8 pt-10 pb-6 text-center">
        <Logo className="size-10" />

        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={slide.id}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: duration.base, ease: EASE_OUT }}
            className="flex min-h-40 flex-col items-center gap-4"
          >
            <slide.icon
              weight="duotone"
              className="size-7 text-muted-foreground"
            />
            <div className="space-y-2">
              <h2 className="font-heading text-lg font-semibold">
                {slide.title()}
              </h2>
              <p className="text-xs leading-relaxed text-muted-foreground">
                {slide.body()}
              </p>
            </div>
            {slide.Extra && <slide.Extra />}
          </motion.div>
        </AnimatePresence>
      </div>

      <div className="flex items-center gap-2 border-t border-border px-4 py-3">
        <div className="flex items-center gap-1.5">
          {slides.map((step, at) => (
            <span
              key={step.id}
              className={cn(
                'size-1.5 transition-colors',
                at === index ? 'bg-ember' : 'bg-muted-foreground/30',
              )}
            />
          ))}
        </div>

        <div className="ml-auto flex items-center gap-1.5">
          {last ? (
            <>
              <Button variant="ghost" size="sm" onClick={() => finish(false)}>
                {m['onboarding.welcome.explore']()}
              </Button>
              <Button size="sm" onClick={() => finish(true)}>
                {m['onboarding.welcome.take_tour']()}
              </Button>
            </>
          ) : (
            <>
              <Button variant="ghost" size="sm" onClick={() => finish(false)}>
                {m['onboarding.welcome.skip']()}
              </Button>
              <Button size="sm" onClick={() => setIndex(index + 1)}>
                {m['onboarding.welcome.next']()}
              </Button>
            </>
          )}
        </div>
      </div>
    </AppDialog>
  );
}
