import { useMutation, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { toast } from 'sonner';

import type { ServerProperty } from '@/api';
import { SearchInput } from '@/components/search-input';
import {
  SavedInput,
  SettingList,
  SettingRow,
} from '@/components/setting-controls';
import { Bone } from '@/components/skeleton';
import { Badge } from '@/components/ui/badge';
import { Switch } from '@/components/ui/switch';
import { matchProperties, propertyControl } from '@/features/servers/lib';
import { m } from '@/paraglide/messages.js';
import { serverMutations, serverQueries } from '@/queries/server';

function PropertyControl({
  property,
  onCommit,
  onSave,
}: {
  property: ServerProperty;
  onCommit: (value: string) => void;
  onSave: (value: string) => void;
}) {
  const id = `property-${property.key}`;
  const disabled = property.managed || property.locked;
  const control = propertyControl(property);

  if (control === 'toggle') {
    return (
      <Switch
        id={id}
        size="sm"
        checked={property.value === 'true'}
        disabled={disabled}
        onCheckedChange={(checked) => onCommit(String(checked === true))}
      />
    );
  }

  return (
    <SavedInput
      id={id}
      trailing
      mono
      type={
        property.secret
          ? 'password'
          : control === 'number'
            ? 'number'
            : undefined
      }
      value={property.value}
      placeholder={property.default}
      disabled={disabled}
      onSave={onSave}
    />
  );
}

/**
 * The server's `server.properties`, key by key, exactly as the daemon lists
 * it for this version — nothing here knows a key by name.
 */
export function PropertiesSettings({
  id,
  running,
}: {
  id: string;
  running: boolean;
}) {
  const properties = useQuery(serverQueries.properties(id));
  const setConfig = useMutation(serverMutations.setConfig(id));
  const [query, setQuery] = useState('');

  if (properties.isPending) return <Bone className="h-64" />;

  const all = properties.data ?? [];
  const shown = matchProperties(all, query);

  return (
    <div>
      <div className="mb-4 flex items-center gap-4">
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
          {all.length === 0
            ? m['server.properties.empty']()
            : m['server.properties.none_match']()}
        </p>
      ) : (
        <SettingList>
          {shown.map((property) => (
            <SettingRow
              key={property.key}
              htmlFor={`property-${property.key}`}
              label={
                <>
                  <span className="font-mono">{property.key}</span>
                  {property.managed && (
                    <Badge variant="outline" className="text-[10px]">
                      {m['server.properties.managed']()}
                    </Badge>
                  )}
                  {property.locked && (
                    <Badge variant="outline" className="text-[10px]">
                      {m['server.properties.locked']()}
                    </Badge>
                  )}
                </>
              }
            >
              <PropertyControl
                property={property}
                onCommit={(value) =>
                  setConfig.mutate({ key: property.key, value })
                }
                onSave={(value) =>
                  setConfig.mutate(
                    { key: property.key, value },
                    { onSuccess: () => toast.success(m['app.toast.saved']()) },
                  )
                }
              />
            </SettingRow>
          ))}
        </SettingList>
      )}
    </div>
  );
}
