import { useMutation } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import type { ConfigEntry } from '@/api';
import { SearchInput } from '@/components/search-input';
import { SavedInput, SwitchRow } from '@/components/setting-controls';
import { Badge } from '@/components/ui/badge';
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from '@/components/ui/field';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import {
  hardcoreLock,
  type Property,
  propertyWrites,
  serverProperties,
} from '@/features/servers/lib';
import { m } from '@/paraglide/messages.js';
import { serverMutations } from '@/queries/server';

const HINTS: Record<string, () => string> = {
  'level-seed': m['entry.settings.seed_hint'],
};

/** `domain.gamemode.*` / `domain.difficulty.*`, else the raw value. */
function choiceLabel(key: string, value: string): string {
  const messages = m as unknown as Record<string, (() => string) | undefined>;
  return messages[`domain.${key}.${value}`]?.() ?? value;
}

function PropertyLabel({ property }: { property: Property }) {
  return (
    <>
      <span className="font-mono">{property.key}</span>
      {property.managed && (
        <Badge variant="outline" className="text-[10px]">
          {m['server.properties.managed']()}
        </Badge>
      )}
    </>
  );
}

function PropertyField({
  property,
  locked,
  onCommit,
  onSave,
}: {
  property: Property;
  locked: boolean;
  onCommit: (value: string) => void;
  onSave: (value: string) => void;
}) {
  const { key, value, control, managed, secret } = property;
  const id = `property-${key}`;
  const disabled = managed || locked;
  const hint = HINTS[key]?.();

  if (control.type === 'toggle') {
    return (
      <SwitchRow
        id={id}
        label={<PropertyLabel property={property} />}
        description={hint}
        checked={value === 'true'}
        disabled={disabled}
        onChange={(checked) => onCommit(String(checked))}
      />
    );
  }

  const items =
    control.type === 'choice'
      ? control.options.map((option) => ({
          value: option,
          label: choiceLabel(key, option),
        }))
      : [];

  return (
    <Field>
      <FieldLabel htmlFor={id} className="font-normal">
        <PropertyLabel property={property} />
      </FieldLabel>
      {control.type === 'choice' ? (
        <Select
          items={items}
          value={value}
          onValueChange={(next) => {
            if (next && next !== value) onCommit(next);
          }}
          disabled={disabled}
        >
          <SelectTrigger id={id} className="max-w-md">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {items.map((item) => (
              <SelectItem key={item.value} value={item.value}>
                {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : (
        <SavedInput
          id={id}
          mono
          type={
            secret
              ? 'password'
              : control.type === 'number'
                ? 'number'
                : undefined
          }
          value={value}
          disabled={disabled}
          onSave={onSave}
        />
      )}
      {hint && <FieldDescription>{hint}</FieldDescription>}
    </Field>
  );
}

/** Every `server.properties` key the server's version carries. */
export function ServerPropertiesTab({
  id,
  config,
  running,
}: {
  id: string;
  config?: ConfigEntry[];
  running: boolean;
}) {
  const setConfig = useMutation(serverMutations.setConfig(id));
  const [query, setQuery] = useState('');

  const properties = serverProperties(config);
  const values = Object.fromEntries(
    properties.map(({ key, value }) => [key, value]),
  );
  const needle = query.trim().toLowerCase();
  const shown = properties.filter(({ key }) => key.includes(needle));

  const write = async (key: string, value: string, confirm: boolean) => {
    for (const entry of propertyWrites(properties, key, value)) {
      await setConfig.mutateAsync(entry);
    }
    if (confirm) toast.success(m['app.toast.saved']());
  };

  return (
    <div className="max-w-2xl">
      <div className="mb-6 flex items-start gap-4">
        <p className="flex-1 text-xs text-muted-foreground">
          {m['server.properties.description']()}
          {running && ` ${m['entry.settings.applies_on_start']()}`}
        </p>
        <SearchInput
          value={query}
          onChange={setQuery}
          placeholder={m['server.properties.search']()}
          className="w-56"
          delay={0}
        />
      </div>
      {shown.length === 0 ? (
        <p className="py-8 text-sm text-muted-foreground">
          {properties.length === 0
            ? m['server.properties.empty']()
            : m['server.properties.none_match']()}
        </p>
      ) : (
        <FieldGroup>
          {shown.map((property) => (
            <PropertyField
              key={property.key}
              property={property}
              locked={hardcoreLock(property.key, values) !== undefined}
              // A failed write already toasts through the query client.
              onCommit={(value) =>
                write(property.key, value, false).catch(() => {})
              }
              onSave={(value) =>
                write(property.key, value, true).catch(() => {})
              }
            />
          ))}
        </FieldGroup>
      )}
    </div>
  );
}
