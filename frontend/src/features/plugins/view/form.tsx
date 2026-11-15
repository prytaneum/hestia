import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';

import type { EntryKind } from '@/api';
import type { FormField, UiNode } from '@/api/types/ui';
import { Button } from '@/components/ui/button';
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { m } from '@/paraglide/messages.js';
import { instanceQueries } from '@/queries/instance';
import { serverQueries } from '@/queries/server';

import { useView } from './context';

type FormNode = Extract<UiNode, { type: 'form' }>;
type Value = string | number | boolean;

function EntryPicker({
  kind,
  value,
  onChange,
}: {
  kind: EntryKind;
  value: string;
  onChange: (id: string) => void;
}) {
  const servers = useQuery({
    ...serverQueries.list(),
    enabled: kind === 'server',
  });
  const instances = useQuery({
    ...instanceQueries.list(),
    enabled: kind === 'instance',
  });
  const entries = (kind === 'server' ? servers.data : instances.data) ?? [];
  const options = entries.map((entry) => ({
    value: entry.id,
    label: entry.name,
  }));
  return <Choice options={options} value={value} onChange={onChange} />;
}

function Choice({
  options,
  value,
  onChange,
}: {
  options: Array<{ value: string; label: string }>;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <Select
      items={options}
      value={value}
      onValueChange={(next) => {
        if (next) onChange(next);
      }}
    >
      <SelectTrigger>
        <SelectValue placeholder={m['plugins.pick']()} />
      </SelectTrigger>
      <SelectContent align="start">
        <SelectGroup>
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
}

function initial(fields: FormField[]): Record<string, Value> {
  return Object.fromEntries(
    fields.map((field) => [
      field.name,
      field.type === 'entry' ? '' : field.value,
    ]),
  );
}

function numberProblem(field: FormField, value: Value): string | null {
  if (field.type !== 'number') return null;
  if (value === '') return m['plugins.field.required']();
  const number = Number(value);
  const outOfRange =
    !Number.isFinite(number) ||
    (field.min !== undefined && number < field.min) ||
    (field.max !== undefined && number > field.max);
  return outOfRange ? m['plugins.field.out_of_range']() : null;
}

export function UiForm({ fields, submit }: FormNode) {
  const { run, pending } = useView();
  const [values, setValues] = useState(() => initial(fields));
  const set = (name: string, value: Value) =>
    setValues((previous) => ({ ...previous, [name]: value }));
  const problems = new Map(
    fields.map((field) => [
      field.name,
      numberProblem(field, values[field.name]),
    ]),
  );
  const blocked = [...problems.values()].some((problem) => problem !== null);

  return (
    <form
      className="max-w-md"
      onSubmit={(event) => {
        event.preventDefault();
        if (blocked) return;
        void run(submit.action, values);
      }}
    >
      <FieldGroup>
        {fields.map((field) => (
          <Field key={field.name}>
            <FieldLabel htmlFor={field.name}>{field.label}</FieldLabel>
            {field.type === 'text' && (
              <Input
                id={field.name}
                placeholder={field.placeholder}
                value={String(values[field.name])}
                onChange={(event) => set(field.name, event.target.value)}
              />
            )}
            {field.type === 'number' && (
              <Input
                id={field.name}
                type="number"
                min={field.min}
                max={field.max}
                step={field.step}
                aria-invalid={problems.get(field.name) !== null}
                value={values[field.name] as number | ''}
                onChange={(event) =>
                  set(
                    field.name,
                    event.target.value === '' ? '' : event.target.valueAsNumber,
                  )
                }
              />
            )}
            {field.type === 'toggle' && (
              <Switch
                id={field.name}
                checked={values[field.name] === true}
                onCheckedChange={(next) => set(field.name, next === true)}
              />
            )}
            {field.type === 'select' && (
              <Choice
                options={field.options}
                value={String(values[field.name])}
                onChange={(next) => set(field.name, next)}
              />
            )}
            {field.type === 'entry' && (
              <EntryPicker
                kind={field.kind}
                value={String(values[field.name])}
                onChange={(next) => set(field.name, next)}
              />
            )}
            {field.help && <FieldDescription>{field.help}</FieldDescription>}
            {problems.get(field.name) && (
              <FieldError>{problems.get(field.name)}</FieldError>
            )}
          </Field>
        ))}
        <div>
          <Button type="submit" disabled={pending || blocked}>
            {submit.label}
          </Button>
        </div>
      </FieldGroup>
    </form>
  );
}
