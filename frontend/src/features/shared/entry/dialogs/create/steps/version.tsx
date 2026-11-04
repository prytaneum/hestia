import { useQuery } from '@tanstack/react-query';
import { useEffect } from 'react';
import type { GameVersion } from '@/api';
import { withForm } from '@/components/form';
import { StaleNotice } from '@/components/offline-state';
import { SearchInput } from '@/components/search-input';
import { Checkbox } from '@/components/ui/checkbox';
import { FieldError } from '@/components/ui/field';
import { createWizardDefaults } from '@/features/shared/entry/lib/schema';
import { m } from '@/paraglide/messages.js';
import { catalogueQueries } from '@/queries/catalogue';
import { useOffline } from '@/queries/net';

import { type Kind, VersionRow } from '../fields';

/** The version list, snapshot toggle, and loader-build picker for a flavor. */
export const VersionStep = withForm({
  defaultValues: createWizardDefaults(''),
  props: {
    kind: 'instance' as Kind,
    flavor: '',
    search: '',
    onSearch: (_value: string) => {},
    showSnapshots: false,
    onShowSnapshots: (_value: boolean) => {},
  },
  render: function VersionStep({
    form,
    kind,
    flavor,
    search,
    onSearch,
    showSnapshots,
    onShowSnapshots,
  }) {
    const selected = form.state.values.version.version;

    const offline = useOffline();
    const versionsQuery = useQuery(catalogueQueries.versions(kind, flavor));
    const versions: GameVersion[] = versionsQuery.data ?? [];

    // Loaders resolve only once a version is chosen, so gate on both.
    const loadersQuery = useQuery({
      ...catalogueQueries.loaders(kind, flavor, selected),
      enabled: flavor !== '' && selected !== '',
    });
    const loaders = loadersQuery.data;

    const q = search.trim().toLowerCase();
    const list = versions.filter((v) => {
      const isRelease = v.kind === 'release';
      if (!showSnapshots && !isRelease) return false;
      if (q && !v.id.toLowerCase().includes(q)) return false;
      return true;
    });

    useEffect(() => {
      const current = form.state.values.version.loaderVersion;
      if (loaders && loaders.length > 0) {
        if (!current || !loaders.includes(current)) {
          form.setFieldValue('version.loaderVersion', loaders[0]);
        }
      } else if (current) {
        form.setFieldValue('version.loaderVersion', '');
      }
    }, [loaders, form]);

    return (
      <div className="flex flex-col gap-3">
        {/* The list answers from the cached catalogue while offline, so say so
            rather than letting a short list read as the whole truth. */}
        {offline && <StaleNotice />}
        <SearchInput
          value={search}
          onChange={onSearch}
          placeholder={m['entry.create.filter_versions']()}
        />

        <label
          htmlFor="wizard-snapshots"
          className="flex w-fit cursor-pointer items-center gap-2 text-xs text-muted-foreground"
        >
          <Checkbox
            id="wizard-snapshots"
            checked={showSnapshots}
            onCheckedChange={(c) => onShowSnapshots(c === true)}
          />
          {m['entry.create.show_snapshots']()}
        </label>

        <form.AppField name="version.version">
          {(field) => {
            const invalid =
              field.state.meta.isTouched && field.state.meta.errors.length > 0;
            return (
              <div className="flex flex-col gap-1.5">
                <div className="max-h-80 divide-y divide-border overflow-y-auto border border-border">
                  {list.length === 0 ? (
                    <p className="px-3 py-6 text-center text-xs text-muted-foreground">
                      {versionsQuery.isPending
                        ? m['app.status.loading']()
                        : m['entry.create.no_versions_match']()}
                    </p>
                  ) : (
                    list.map((v) => (
                      <VersionRow
                        key={v.id}
                        id={v.id}
                        snapshot={v.kind !== 'release'}
                        selected={field.state.value === v.id}
                        onSelect={() => field.handleChange(v.id)}
                      />
                    ))
                  )}
                </div>
                {invalid && <FieldError errors={field.state.meta.errors} />}
              </div>
            );
          }}
        </form.AppField>

        {loaders && loaders.length > 0 && (
          <form.AppField name="version.loaderVersion">
            {(field) => (
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">
                  {m['app.label.loader']()}
                </span>
                <field.SelectField
                  options={loaders.map((l) => ({ value: l, label: l }))}
                  triggerClassName="w-40"
                />
              </div>
            )}
          </form.AppField>
        )}
      </div>
    );
  },
});
