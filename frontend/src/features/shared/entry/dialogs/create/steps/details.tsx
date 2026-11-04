import { ExternalLink } from '@/components/external-link';
import { withForm } from '@/components/form';
import { Checkbox } from '@/components/ui/checkbox';
import { FieldError } from '@/components/ui/field';
import { createWizardDefaults } from '@/features/shared/entry/lib/schema';
import { m } from '@/paraglide/messages.js';
import { useMemoryRange } from '@/queries/host';

import {
  DIFFICULTIES,
  GAMEMODES,
  type Kind,
  options,
  PropToggle,
  SectionHeader,
} from '../fields';

/** The details step: name + memory, plus the server.properties block. */
export const DetailsStep = withForm({
  defaultValues: createWizardDefaults(''),
  props: { kind: 'instance' as Kind },
  render: function DetailsStep({ form, kind }) {
    const memoryRange = useMemoryRange();
    return (
      <div className="flex flex-col gap-4">
        <form.Subscribe
          selector={(s) =>
            [s.values.flavor.flavor, s.values.version.version] as const
          }
        >
          {([flavor, version]) => (
            <form.AppField name="details.name">
              {(field) => (
                <field.TextField
                  label={m['app.label.name']()}
                  placeholder={`${flavor}-${version}`}
                  description={m['entry.create.name_hint']()}
                />
              )}
            </form.AppField>
          )}
        </form.Subscribe>

        <form.AppField name="details.memory">
          {(field) => (
            <field.SliderField
              label={m['app.label.memory']()}
              formatValue={(v: number) => m['entry.create.gb']({ value: v })}
              min={memoryRange.min}
              max={memoryRange.max}
              step={1}
            />
          )}
        </form.AppField>

        {kind === 'server' && (
          <>
            <SectionHeader>
              {m['entry.create.properties.title']()}
            </SectionHeader>

            <form.AppField name="details.motd">
              {(field) => (
                <field.TextField label={m['entry.create.properties.motd']()} />
              )}
            </form.AppField>

            <form.AppField name="details.seed">
              {(field) => (
                <field.TextField
                  label={m['entry.create.properties.seed']()}
                  placeholder={m['entry.create.properties.seed_random']()}
                />
              )}
            </form.AppField>

            {/* Hardcore forces difficulty=hard and gamemode=survival, so both
                selects are pinned and disabled while it is on — matching the
                daemon's invariant and Minecraft's own create-world screen. */}
            <form.Subscribe selector={(s) => s.values.details.hardcore}>
              {(hardcore) => (
                <div className="grid gap-4 sm:grid-cols-2">
                  <form.AppField name="details.gamemode">
                    {(field) => (
                      <field.SelectField
                        label={m['entry.create.properties.gamemode']()}
                        options={options(GAMEMODES)}
                        triggerClassName="w-full"
                        disabled={hardcore}
                      />
                    )}
                  </form.AppField>
                  <form.AppField name="details.difficulty">
                    {(field) => (
                      <field.SelectField
                        label={m['entry.create.properties.difficulty']()}
                        options={options(DIFFICULTIES)}
                        triggerClassName="w-full"
                        disabled={hardcore}
                      />
                    )}
                  </form.AppField>
                </div>
              )}
            </form.Subscribe>

            <div className="grid gap-4 sm:grid-cols-2">
              <form.AppField name="details.maxPlayers">
                {(field) => (
                  <field.TextField
                    label={m['entry.create.properties.max_players']()}
                    type="number"
                  />
                )}
              </form.AppField>
              <form.AppField name="details.port">
                {(field) => (
                  <field.TextField
                    label={m['entry.create.properties.port']()}
                    type="number"
                    placeholder={m['entry.create.properties.port_auto']()}
                    description={m['entry.create.properties.port_hint']()}
                  />
                )}
              </form.AppField>
            </div>

            <div className="grid grid-cols-2 gap-4 pt-1">
              <form.AppField name="details.hardcore">
                {(field) => (
                  <PropToggle
                    id="prop-hardcore"
                    label={m['entry.create.properties.hardcore']()}
                    checked={field.state.value}
                    onChange={(checked) => {
                      field.handleChange(checked);
                      if (checked) {
                        form.setFieldValue('details.gamemode', 'survival');
                        form.setFieldValue('details.difficulty', 'hard');
                      }
                    }}
                  />
                )}
              </form.AppField>
              <form.AppField name="details.onlineMode">
                {(field) => (
                  <PropToggle
                    id="prop-online"
                    label={m['entry.create.properties.online_mode']()}
                    checked={field.state.value}
                    onChange={field.handleChange}
                  />
                )}
              </form.AppField>
            </div>

            <form.AppField name="details.eula">
              {(field) => {
                const invalid =
                  field.state.meta.isTouched &&
                  field.state.meta.errors.length > 0;
                return (
                  <div className="flex flex-col gap-1.5">
                    <label
                      htmlFor={field.name}
                      className="flex cursor-pointer items-center gap-2.5 border border-border px-3 py-2.5"
                    >
                      <Checkbox
                        id={field.name}
                        checked={field.state.value}
                        onCheckedChange={(c) => field.handleChange(c === true)}
                      />
                      <span className="text-xs text-muted-foreground">
                        {m['entry.create.eula.before']()}{' '}
                        <ExternalLink
                          href="https://aka.ms/MinecraftEULA"
                          className="text-foreground underline underline-offset-2"
                        >
                          {m['entry.create.eula.link']()}
                        </ExternalLink>
                        {m['entry.create.eula.after']()}
                      </span>
                    </label>
                    {invalid && <FieldError errors={field.state.meta.errors} />}
                  </div>
                );
              }}
            </form.AppField>
          </>
        )}
      </div>
    );
  },
});
