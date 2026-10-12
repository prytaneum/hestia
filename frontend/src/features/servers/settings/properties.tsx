import { useState } from 'react';

import type { ConfigEntry } from '@/api';
import { Empty } from '@/components/empty';
import { SearchInput } from '@/components/search-input';
import { Badge } from '@/components/ui/badge';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import {
  hardcoreLock,
  type Property,
  propertyEdits,
  serverProperties,
} from '@/features/servers/lib';
import {
  SaveBar,
  type SetConfig,
  SettingsSection,
  saveConfig,
} from '@/features/shared/entry/settings';
import { m } from '@/paraglide/messages.js';

const HINTS: Record<string, () => string> = {
  'level-seed': m['entry.settings.seed_hint'],
};

/** `domain.gamemode.*` / `domain.difficulty.*`, else the raw value. */
function choiceLabel(key: string, value: string): string {
  const messages = m as unknown as Record<string, (() => string) | undefined>;
  return messages[`domain.${key}.${value}`]?.() ?? value;
}

function PropertyControl({
  property,
  value,
  locked,
  onChange,
}: {
  property: Property;
  value: string;
  locked: boolean;
  onChange: (value: string) => void;
}) {
  const { key, control, managed, secret } = property;
  const disabled = managed || locked;
  const id = `property-${key}`;

  switch (control.type) {
    case 'toggle':
      return (
        <Switch
          id={id}
          checked={value === 'true'}
          onCheckedChange={(checked) => onChange(String(checked))}
          disabled={disabled}
        />
      );
    case 'choice': {
      const items = control.options.map((option) => ({
        value: option,
        label: choiceLabel(key, option),
      }));
      return (
        <Select
          items={items}
          value={value}
          onValueChange={(next) => onChange(next ?? value)}
          disabled={disabled}
        >
          <SelectTrigger id={id} className="w-40">
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
      );
    }
    default:
      return (
        <Input
          id={id}
          type={
            secret ? 'password' : control.type === 'number' ? 'number' : 'text'
          }
          value={value}
          onChange={(e) => onChange(e.target.value)}
          disabled={disabled}
          className={
            control.type === 'number' ? 'w-28 font-mono' : 'w-64 font-mono'
          }
        />
      );
  }
}

/** Every `server.properties` key the server's version carries. */
export function PropertiesSection({
  config,
  running,
  setConfig,
}: {
  config?: ConfigEntry[];
  running: boolean;
  setConfig: SetConfig;
}) {
  const [query, setQuery] = useState('');
  const [draft, setDraft] = useState<Record<string, string>>({});

  const properties = serverProperties(config);
  const values = {
    ...Object.fromEntries(properties.map(({ key, value }) => [key, value])),
    ...draft,
  };
  const edits = propertyEdits(properties, draft);
  const needle = query.trim().toLowerCase();
  const shown = properties.filter(({ key }) => key.includes(needle));

  const change = (property: Property, value: string) => {
    const next = { ...draft, [property.key]: value };
    if (property.key === 'hardcore' && value === 'true') {
      for (const other of properties) {
        const pinned = hardcoreLock(other.key, next);
        if (pinned && other.control.type === 'choice') next[other.key] = pinned;
      }
    }
    setDraft(next);
  };

  const save = async () => {
    if (await saveConfig(setConfig, edits)) setDraft({});
  };

  return (
    <>
      <SettingsSection
        title={m['server.properties.title']()}
        description={m['server.properties.description']()}
        className="max-w-2xl"
        actions={
          <SearchInput
            value={query}
            onChange={setQuery}
            placeholder={m['server.properties.search']()}
            className="w-56"
            delay={0}
          />
        }
      >
        {shown.length === 0 ? (
          <Empty>
            {properties.length === 0
              ? m['server.properties.empty']()
              : m['server.properties.none_match']()}
          </Empty>
        ) : (
          <ul className="divide-y divide-border border border-border">
            {shown.map((property) => {
              const hint = HINTS[property.key];
              const edited =
                property.key in draft && draft[property.key] !== property.value;
              return (
                <li
                  key={property.key}
                  className="flex min-h-11 items-center gap-4 px-3 py-1.5"
                >
                  <label
                    htmlFor={`property-${property.key}`}
                    className="min-w-0 flex-1"
                  >
                    <span className="flex items-center gap-2 font-mono text-xs">
                      <span className={edited ? 'text-amber' : undefined}>
                        {property.key}
                      </span>
                      {property.managed && (
                        <Badge variant="outline" className="text-[10px]">
                          {m['server.properties.managed']()}
                        </Badge>
                      )}
                    </span>
                    {hint && (
                      <span className="block text-[11px] text-muted-foreground">
                        {hint()}
                      </span>
                    )}
                  </label>
                  <PropertyControl
                    property={property}
                    value={values[property.key]}
                    locked={hardcoreLock(property.key, values) !== undefined}
                    onChange={(value) => change(property, value)}
                  />
                </li>
              );
            })}
          </ul>
        )}
      </SettingsSection>
      <SaveBar
        dirty={edits.length > 0}
        pending={setConfig.isPending}
        running={running}
        onSave={save}
        onDiscard={() => setDraft({})}
      />
    </>
  );
}
