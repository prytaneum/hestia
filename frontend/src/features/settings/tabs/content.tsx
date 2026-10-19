import { useQuery } from '@tanstack/react-query';

import { Badge } from '@/components/ui/badge';
import { Field, FieldDescription, FieldLabel } from '@/components/ui/field';
import {
  ModpackSettings,
  SavedInput,
  Setting,
  SettingsSection,
} from '@/features/settings/components';
import { useConfig } from '@/features/settings/use-config';
import { m } from '@/paraglide/messages.js';
import { contentQueries } from '@/queries/content';

export function ContentTab() {
  const { entries, pending, save, commit } = useConfig();

  return (
    <>
      <SettingsSection
        group="content-sources"
        legend={m['settings.content_sources']()}
      >
        <Setting id="curseforge-key">
          <CurseForgeKey
            value={entries.content?.['curseforge-key'] ?? ''}
            onSave={(value) => save('content.curseforge-key', value)}
          />
        </Setting>
      </SettingsSection>

      <SettingsSection
        group="modpack"
        legend={m['settings.modpack.title']()}
        description={m['settings.modpack.hint']()}
      >
        <Setting id="modpack">
          <ModpackSettings
            config={entries.modpack ?? {}}
            pending={pending}
            onCommit={commit}
          />
        </Setting>
      </SettingsSection>
    </>
  );
}

/**
 * The CurseForge key, plus whether the daemon now counts the source as one it
 * can serve from — the source list is the authority, so a typo shows as still
 * needing a key rather than as a silent success.
 */
function CurseForgeKey({
  value,
  onSave,
}: {
  value: string;
  onSave: (value: string) => Promise<unknown>;
}) {
  const sources = useQuery(contentQueries.sources());
  const ready = (sources.data ?? []).some((s) => s.id === 'curseforge');
  return (
    <Field>
      <FieldLabel htmlFor="curseforge-key" className="gap-2">
        {m['settings.curseforge_key']()}
        <Badge variant={ready ? 'secondary' : 'outline'}>
          {ready
            ? m['settings.source_ready']()
            : m['settings.source_needs_key']()}
        </Badge>
      </FieldLabel>
      <SavedInput
        id="curseforge-key"
        type="password"
        mono
        value={value}
        onSave={onSave}
      />
      <FieldDescription>{m['settings.curseforge_key_hint']()}</FieldDescription>
    </Field>
  );
}
