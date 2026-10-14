import type { Flavor } from '@/api';
import { DialogBody } from '@/components/dialog';
import { ExternalLink } from '@/components/external-link';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import { kindInfo } from '@/features/shared/content/lib';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

export type Kind = 'server' | 'instance';
export type Step = 'flavor' | 'version' | 'details';

export const STEPS: Step[] = ['flavor', 'version', 'details'];

// biome-ignore lint/suspicious/noExplicitAny: the wizard form's generic type is internal to TanStack Form.
export type WizardForm = any;

export const GAMEMODES: Array<{ value: string; label: () => string }> = [
  { value: 'survival', label: m['domain.gamemode.survival'] },
  { value: 'creative', label: m['domain.gamemode.creative'] },
  { value: 'adventure', label: m['domain.gamemode.adventure'] },
  { value: 'spectator', label: m['domain.gamemode.spectator'] },
];
export const DIFFICULTIES: Array<{ value: string; label: () => string }> = [
  { value: 'peaceful', label: m['domain.difficulty.peaceful'] },
  { value: 'easy', label: m['domain.difficulty.easy'] },
  { value: 'normal', label: m['domain.difficulty.normal'] },
  { value: 'hard', label: m['domain.difficulty.hard'] },
];

export const options = (items: Array<{ value: string; label: () => string }>) =>
  items.map((o) => ({ value: o.value, label: o.label() }));

export const STEP_HINTS: Record<Step, (kind: Kind) => string> = {
  flavor: (kind) =>
    kind === 'server'
      ? m['entry.create.hint.flavor_server']()
      : m['entry.create.hint.flavor_instance'](),
  version: () => m['entry.create.hint.version'](),
  details: (kind) =>
    kind === 'server'
      ? m['entry.create.hint.details_server']()
      : m['entry.create.hint.details_instance'](),
};

export function StepForm({
  onSubmit,
  footer,
  children,
}: {
  onSubmit: () => void;
  footer: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <form
      className="flex min-h-0 flex-1 flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onSubmit();
      }}
    >
      <DialogBody>{children}</DialogBody>
      {footer}
    </form>
  );
}

export function SectionHeader({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-2.5 pt-1">
      <span className="text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
        {children}
      </span>
      <div className="h-px flex-1 bg-border" />
    </div>
  );
}

export function PropToggle({
  id,
  label,
  checked,
  onChange,
}: {
  id: string;
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label
      htmlFor={id}
      className="flex cursor-pointer items-center gap-2.5 border border-border px-3 py-2 text-xs font-medium leading-none transition-colors hover:bg-muted/40"
    >
      <Checkbox
        id={id}
        checked={checked}
        onCheckedChange={(c) => onChange(c === true)}
      />
      {label}
    </label>
  );
}

export function FlavorOption({
  flavor,
  selected,
  onSelect,
}: {
  flavor: Flavor;
  selected: boolean;
  onSelect: () => void;
}) {
  const summary = flavorSummary(flavor);
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      className={cn(
        'flex flex-col items-start gap-0.5 p-3 text-left ring-1 transition-colors outline-none focus-visible:ring-ring',
        selected
          ? 'bg-muted ring-ember'
          : 'ring-border hover:bg-muted/60 hover:ring-foreground/20',
      )}
    >
      <span className="flex w-full items-center gap-2">
        <span className="flex-1 text-sm font-medium">{flavor.name}</span>
        {(flavor.accepts ?? []).map((kind) => (
          <Badge key={kind} variant="outline" className="text-[10px]">
            {kindInfo[kind].label()}
          </Badge>
        ))}
      </span>
      {summary && (
        <span className="text-xs text-muted-foreground">{summary}</span>
      )}
      {(flavor.requires ?? []).map((requirement) => (
        <span key={requirement.name} className="text-xs text-amber">
          {m['domain.flavor.requires']({ name: requirement.name })}{' '}
          <ExternalLink
            href={requirement.url}
            className="underline"
            onClick={(event) => event.stopPropagation()}
          >
            {requirement.url}
          </ExternalLink>
        </span>
      ))}
    </button>
  );
}

export function VersionRow({
  id,
  snapshot,
  selected,
  onSelect,
}: {
  id: string;
  snapshot: boolean;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      className={cn(
        'flex w-full items-center gap-2 px-3 py-2 text-left outline-none transition-colors focus-visible:ring-1 focus-visible:ring-ring focus-visible:ring-inset',
        selected ? 'bg-muted text-foreground' : 'hover:bg-muted/50',
      )}
    >
      <span
        className={cn(
          'size-1.5 rounded-full',
          selected ? 'bg-ember' : 'bg-transparent',
        )}
      />
      <span className="flex-1 font-mono text-xs">{id}</span>
      {snapshot && (
        <Badge variant="outline" className="text-[10px]">
          {m['entry.create.snapshot']()}
        </Badge>
      )}
    </button>
  );
}

/**
 * A flavor's blurb: the localised message for a known id, else the daemon's own
 * summary. So a flavor the daemon ships renders immediately with nothing added
 * here, and a locale that has translated it keeps its translation.
 */
export function flavorSummary(flavor: Flavor): string {
  const messages = m as unknown as Record<string, (() => string) | undefined>;
  return (
    messages[`domain.flavor.${flavor.id}.summary`]?.() ?? flavor.summary ?? ''
  );
}
