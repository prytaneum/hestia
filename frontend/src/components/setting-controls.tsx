import { type ReactNode, useState } from 'react';

import { SaveButton, SaveStatus } from '@/components/save-button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldLegend,
  FieldSet,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { useSaveState } from '@/hooks/use-save-state';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

/**
 * The controls every settings surface is built from — the launcher's own and
 * an entry's. A binary is a switch because it lands the moment it moves; a
 * typed value is saved on purpose, since nothing else about a text field says
 * the daemon took it.
 */

/** A legend over the fields it names. */
export function SettingGroup({
  legend,
  description,
  children,
}: {
  legend: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <FieldSet>
      <FieldLegend>{legend}</FieldLegend>
      <FieldGroup>
        {description && <FieldDescription>{description}</FieldDescription>}
        {children}
      </FieldGroup>
    </FieldSet>
  );
}

/** The bordered list a group's rows sit in. */
export function SettingList({ children }: { children: ReactNode }) {
  return (
    <div className="divide-y divide-border border border-border">
      {children}
    </div>
  );
}

/**
 * One setting as a row: what it is on the left, the one control that changes
 * it on the right. `htmlFor` ties the title to that control as its label.
 */
export function SettingRow({
  htmlFor,
  label,
  description,
  children,
}: {
  htmlFor?: string;
  label: ReactNode;
  description?: ReactNode;
  children: ReactNode;
}) {
  const Title = htmlFor ? 'label' : 'div';
  return (
    <div className="flex min-h-11 items-center gap-4 px-3 py-2">
      <div className="min-w-0 flex-1">
        <Title htmlFor={htmlFor} className="flex items-center gap-2 text-xs">
          {label}
        </Title>
        {description && (
          <div className="text-[11px] text-muted-foreground">{description}</div>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>
  );
}

/** A binary setting that takes effect the moment it moves. */
export function SwitchRow({
  id,
  label,
  description,
  checked,
  disabled,
  onChange,
}: {
  id: string;
  label: ReactNode;
  description?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel htmlFor={id} className="font-normal">
          {label}
        </FieldLabel>
        {description && <FieldDescription>{description}</FieldDescription>}
      </FieldContent>
      <Switch
        id={id}
        checked={checked}
        disabled={disabled}
        onCheckedChange={(next) => onChange(next === true)}
      />
    </Field>
  );
}

/**
 * A text setting that commits on purpose: Save appears once the value differs
 * and the button confirms the write inline. `onSave` returns the write so the
 * button can follow it; a failed write keeps the typed value. `confirm` gates the save behind a
 * dialog, for a value whose change moves the whole daemon.
 */
export function SavedInput({
  id,
  value,
  onSave,
  type,
  mono,
  placeholder,
  disabled,
  trailing,
  confirm,
}: {
  id: string;
  value: string;
  onSave: (value: string) => Promise<unknown>;
  type?: 'password' | 'number';
  mono?: boolean;
  placeholder?: string;
  disabled?: boolean;
  /** Sits at the end of a row: a fixed width, with Save appearing before it. */
  trailing?: boolean;
  confirm?: { title: string; description: string };
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const [seenValue, setSeenValue] = useState(value);
  const { state, run } = useSaveState();
  const current = draft ?? value;
  const dirty = current.trim() !== value;

  if (value !== seenValue) {
    setSeenValue(value);
    setDraft(null);
  }

  const save = () => {
    if (dirty) void run(() => onSave(current.trim()));
  };

  return (
    <div
      className={cn(
        'flex items-center gap-2',
        trailing ? 'flex-row-reverse' : 'max-w-md',
      )}
    >
      <Input
        id={id}
        type={type}
        autoComplete={type === 'password' ? 'off' : undefined}
        className={cn(
          trailing ? (type === 'number' ? 'w-24' : 'w-64') : 'flex-1',
          mono && 'font-mono',
        )}
        placeholder={placeholder}
        value={current}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && dirty && !confirm) save();
        }}
      />
      <SaveStatus state={state} />
      {(dirty || state !== 'idle') &&
        (confirm ? (
          <ConfirmDialog
            trigger={<SaveButton variant="outline" state={state} />}
            title={confirm.title}
            description={confirm.description}
            confirmLabel={m['app.action.save']()}
            onConfirm={save}
          />
        ) : (
          <SaveButton variant="outline" state={state} onClick={save} />
        ))}
    </div>
  );
}
