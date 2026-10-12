import { PlusIcon, XIcon } from '@phosphor-icons/react';
import { type ReactNode, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { m } from '@/paraglide/messages.js';

/** A bordered list of free-form entries, each removable, with an add row. */
export function RowList({
  label,
  description,
  placeholder,
  values,
  pending,
  onChange,
}: {
  label: string;
  description?: string;
  placeholder: string;
  values: string[];
  pending?: boolean;
  onChange: (values: string[]) => void;
}) {
  return (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      {description && <FieldDescription>{description}</FieldDescription>}
      <div className="divide-y divide-border border border-border">
        {values.length === 0 ? (
          <p className="px-3 py-2 text-xs text-muted-foreground">
            {m['settings.list.empty']()}
          </p>
        ) : (
          values.map((value) => (
            <ValueRow
              key={value}
              value={value}
              pending={pending}
              onRemove={() => onChange(values.filter((v) => v !== value))}
            />
          ))
        )}
        <AddRow
          placeholder={placeholder}
          label={m['content.add']()}
          pending={pending}
          onAdd={(value) => {
            if (!values.includes(value)) onChange([...values, value]);
          }}
        />
      </div>
    </Field>
  );
}

/** One entry of a bordered list, removable, optionally tagged. */
export function ValueRow({
  value,
  badge,
  pending,
  onRemove,
}: {
  value: string;
  badge?: ReactNode;
  pending?: boolean;
  onRemove: () => void;
}) {
  return (
    <div className="flex items-center gap-2 px-3 py-1.5">
      <span className="min-w-0 flex-1 truncate font-mono text-xs">{value}</span>
      {badge}
      <Button
        variant="ghost"
        size="icon-sm"
        aria-label={m['app.action.remove']()}
        disabled={pending}
        onClick={onRemove}
      >
        <XIcon className="size-3.5" />
      </Button>
    </div>
  );
}

/** The last row of a bordered list: type a value, add it. */
export function AddRow({
  placeholder,
  label,
  pending,
  onAdd,
}: {
  placeholder: string;
  label: string;
  pending?: boolean;
  onAdd: (value: string) => void;
}) {
  const [draft, setDraft] = useState('');

  const add = () => {
    const value = draft.trim();
    if (!value) return;
    setDraft('');
    onAdd(value);
  };

  return (
    <div className="flex items-center gap-2 bg-muted/30 px-2 py-1.5">
      <Input
        value={draft}
        placeholder={placeholder}
        className="h-7 flex-1 border-transparent bg-transparent font-mono text-xs shadow-none"
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') add();
        }}
      />
      <Button
        variant="ghost"
        size="xs"
        data-icon="inline-start"
        disabled={pending || draft.trim().length === 0}
        onClick={add}
      >
        <PlusIcon weight="bold" className="size-3.5" />
        {label}
      </Button>
    </div>
  );
}
