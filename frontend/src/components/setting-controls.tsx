import { type ReactNode, useState } from 'react';

import { Button } from '@/components/ui/button';
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
 * and the write confirms with a toast. `confirm` gates the save behind a
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
  confirm,
}: {
  id: string;
  value: string;
  onSave: (value: string) => void;
  type?: 'password' | 'number';
  mono?: boolean;
  placeholder?: string;
  disabled?: boolean;
  confirm?: { title: string; description: string };
}) {
  const [draft, setDraft] = useState<string | null>(null);
  const current = draft ?? value;
  const dirty = current.trim() !== value;

  const save = () => {
    setDraft(null);
    onSave(current.trim());
  };

  return (
    <div className="flex max-w-md items-center gap-2">
      <Input
        id={id}
        type={type}
        autoComplete={type === 'password' ? 'off' : undefined}
        className={cn('flex-1', mono && 'font-mono')}
        placeholder={placeholder}
        value={current}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && dirty && !confirm) save();
        }}
      />
      {dirty &&
        (confirm ? (
          <ConfirmDialog
            trigger={
              <Button variant="outline" size="sm">
                {m['app.action.save']()}
              </Button>
            }
            title={confirm.title}
            description={confirm.description}
            confirmLabel={m['app.action.save']()}
            onConfirm={save}
          />
        ) : (
          <Button variant="outline" size="sm" onClick={save}>
            {m['app.action.save']()}
          </Button>
        ))}
    </div>
  );
}
