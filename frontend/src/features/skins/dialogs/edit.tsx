import { UploadSimpleIcon } from '@phosphor-icons/react';
import { useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import type { Cape, Skin, SkinVariant } from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { Field, FieldLabel } from '@/components/ui/field';
import { Input } from '@/components/ui/input';
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group';
import { CapeCard, CapeGrid, SkinModel } from '@/features/skins/components';
import {
  readTextureFile,
  readTexturePath,
  TEXTURE_EXTENSIONS,
} from '@/features/skins/lib';
import { useDropzone } from '@/hooks/use-dropzone';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

export interface SkinDraft {
  name: string;
  variant: SkinVariant;
  /** Add mode only: the texture data URL to upload. */
  texture: string;
  /** The account cape to wear; `undefined` clears it. */
  capeId?: string;
}

// The cape choice edits the account's equipped cape — never stored per skin.
export function EditSkinDialog({
  open,
  onOpenChange,
  skin,
  initialTexture,
  capes,
  equippedCapeId,
  saving,
  onSave,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The skin being edited, or `null` when adding a new one. */
  skin: Skin | null;
  /** Add mode: the texture data URL read from the picked file. */
  initialTexture?: string;
  /** The account's owned capes; empty hides the cape choice. */
  capes: Cape[];
  equippedCapeId?: string;
  saving: boolean;
  onSave: (draft: SkinDraft) => void;
}) {
  const [name, setName] = useState('');
  const [variant, setVariant] = useState<SkinVariant>('classic');
  const [texture, setTexture] = useState('');
  const [capeId, setCapeId] = useState<string | undefined>(undefined);
  const fileRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    setName(skin?.name ?? '');
    setVariant(skin?.variant ?? 'classic');
    setTexture(skin?.texture ?? initialTexture ?? '');
    setCapeId(equippedCapeId);
  }, [open, skin, initialTexture, equippedCapeId]);

  const adding = skin === null;
  const canSave = texture !== '' && name.trim() !== '' && !saving;

  const pickTexture = async (file: File | undefined) => {
    if (!file?.type.includes('png')) return;
    setTexture(await readTextureFile(file));
  };

  const { getRootProps, isDragActive } = useDropzone<HTMLDivElement>({
    accept: TEXTURE_EXTENSIONS,
    multiple: false,
    disabled: !adding,
    onDrop: ([path]) => {
      readTexturePath(path)
        .then(setTexture)
        .catch(() => toast.error(m['skin.texture_unreadable']()));
    },
  });

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      size="xl"
      title={adding ? m['skin.add']() : m['skin.edit']()}
      description={
        adding ? m['skin.add_description']() : m['skin.edit_description']()
      }
    >
      <DialogBody {...getRootProps()} className="flex gap-4">
        <div
          className={cn(
            'grid w-48 shrink-0 place-items-center self-stretch bg-muted/40 ring-1 ring-border',
            isDragActive && 'ring-2 ring-primary',
          )}
        >
          {texture ? (
            <SkinModel
              texture={texture}
              capeTexture={capes.find((c) => c.id === capeId)?.texture}
              variant={variant}
              width={192}
              height={288}
            />
          ) : (
            <span className="px-4 text-center text-xs text-muted-foreground">
              {m['skin.upload_hint']()}
            </span>
          )}
        </div>

        <div className="flex min-w-0 flex-1 flex-col gap-4">
          <Field>
            <FieldLabel htmlFor="skin-name">{m['app.label.name']()}</FieldLabel>
            <Input
              id="skin-name"
              value={name}
              placeholder={m['skin.name_placeholder']()}
              onChange={(e) => setName(e.target.value)}
            />
          </Field>

          {adding && (
            <Field>
              <FieldLabel>{m['skin.texture']()}</FieldLabel>
              <input
                ref={fileRef}
                type="file"
                accept="image/png"
                className="hidden"
                onChange={(e) => {
                  pickTexture(e.target.files?.[0]);
                  e.target.value = '';
                }}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="w-fit"
                data-icon="inline-start"
                onClick={() => fileRef.current?.click()}
              >
                <UploadSimpleIcon />
                {texture
                  ? m['skin.replace_texture']()
                  : m['skin.upload_texture']()}
              </Button>
            </Field>
          )}

          <Field>
            <FieldLabel>{m['skin.arm_style']()}</FieldLabel>
            <ToggleGroup
              variant="outline"
              size="sm"
              value={[variant]}
              onValueChange={(vals: string[]) => {
                const next = vals[vals.length - 1];
                if (next) setVariant(next as SkinVariant);
              }}
            >
              <ToggleGroupItem value="classic">
                {m['skin.wide']()}
              </ToggleGroupItem>
              <ToggleGroupItem value="slim">{m['skin.slim']()}</ToggleGroupItem>
            </ToggleGroup>
          </Field>

          {capes.length > 0 && (
            <Field>
              <FieldLabel>{m['skin.cape']()}</FieldLabel>
              <CapeGrid>
                <CapeCard
                  label={m['skin.no_cape']()}
                  equipped={capeId === undefined}
                  onEquip={() => setCapeId(undefined)}
                />
                {capes.map((cape) => (
                  <CapeCard
                    key={cape.id}
                    label={cape.name}
                    texture={cape.texture}
                    equipped={capeId === cape.id}
                    onEquip={() => setCapeId(cape.id)}
                  />
                ))}
              </CapeGrid>
            </Field>
          )}
        </div>
      </DialogBody>

      <DialogFooter>
        <Button
          variant="outline"
          disabled={saving}
          onClick={() => onOpenChange(false)}
        >
          {m['app.action.cancel']()}
        </Button>
        <Button
          disabled={!canSave}
          className="bg-ember text-ember-foreground hover:bg-ember/90"
          onClick={() =>
            onSave({ name: name.trim(), variant, texture, capeId })
          }
        >
          {saving
            ? m['skin.saving']()
            : adding
              ? m['skin.add']()
              : m['app.action.save_skin']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}
