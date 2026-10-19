import {
  CheckCircleIcon,
  CircleDashedIcon,
  XIcon,
} from '@phosphor-icons/react';
import { motion } from 'motion/react';

import { Button } from '@/components/ui/button';
import { useLaunch } from '@/features/instances/hooks';
import { listContainer, listItem } from '@/lib/motion';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { useInstances } from '@/queries/instance';
import { useServers } from '@/queries/server';

import { useOnboarding } from '../state';
import { progress, type TaskAction, type TaskProgress } from './tasks';

export function GettingStarted({
  onCreate,
}: {
  onCreate: (entry: 'instance' | 'server') => void;
}) {
  const onboarding = useOnboarding();
  const accounts = useAccounts();
  const instances = useInstances();
  const servers = useServers();
  const { launch } = useLaunch();

  const steps = progress({
    signedIn: accounts.signedIn,
    instances: instances.data ?? [],
    servers: servers.data ?? [],
  });
  const complete = steps.filter((step) => step.done).length;
  const current = steps.find((step) => !step.done && !step.blocked);

  const settled =
    onboarding.ready &&
    accounts.ready &&
    !instances.isPending &&
    !servers.isPending;
  if (!settled || onboarding.checklistDismissed || complete === steps.length) {
    return null;
  }

  const perform = (action: TaskAction) => {
    switch (action.kind) {
      case 'sign-in':
        accounts.login.mutate();
        break;
      case 'create':
        onCreate(action.entry);
        break;
      case 'play': {
        const instance = (instances.data ?? [])[0];
        if (instance) launch(instance.id);
        break;
      }
    }
  };

  return (
    <section className="border border-border">
      <div className="flex items-center gap-3 border-b border-border px-4 py-2.5">
        <h2 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">
          {m['onboarding.checklist.title']()}
        </h2>
        <span className="font-mono text-[11px] text-muted-foreground">
          {m['onboarding.checklist.progress']({
            current: complete,
            total: steps.length,
          })}
        </span>
        <Button
          variant="ghost"
          size="icon-xs"
          className="ml-auto"
          aria-label={m['onboarding.checklist.dismiss']()}
          onClick={() => onboarding.update({ checklistDismissed: true })}
        >
          <XIcon />
        </Button>
      </div>

      <motion.ul
        variants={listContainer(steps.length)}
        initial="hidden"
        animate="show"
        className="divide-y divide-border"
      >
        {steps.map((step) => (
          <Row
            key={step.task.id}
            step={step}
            active={step.task.id === current?.task.id}
            busy={step.task.action.kind === 'sign-in' && accounts.signingIn}
            onAct={() => perform(step.task.action)}
          />
        ))}
      </motion.ul>
    </section>
  );
}

function Row({
  step,
  active,
  busy,
  onAct,
}: {
  step: TaskProgress;
  active: boolean;
  busy: boolean;
  onAct: () => void;
}) {
  const { task, done, blocked } = step;
  const Glyph = done ? CheckCircleIcon : CircleDashedIcon;

  return (
    <motion.li
      variants={listItem}
      className={cn(
        'flex items-center gap-3 px-4 py-2.5',
        blocked && !done && 'opacity-45',
      )}
    >
      <Glyph
        weight={done ? 'fill' : 'regular'}
        className={cn(
          'size-4 shrink-0',
          done ? 'text-ember' : 'text-muted-foreground/60',
        )}
      />
      <div className="min-w-0">
        <p
          className={cn(
            'text-xs font-medium',
            done && 'text-muted-foreground line-through',
          )}
        >
          {task.label()}
        </p>
        {!done && (
          <p className="text-[11px] text-muted-foreground">{task.hint()}</p>
        )}
      </div>
      {active && (
        <Button size="xs" className="ml-auto" disabled={busy} onClick={onAct}>
          {m['onboarding.checklist.go']()}
        </Button>
      )}
    </motion.li>
  );
}
