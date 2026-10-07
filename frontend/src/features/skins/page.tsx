import { PlusIcon, WarningCircleIcon } from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useRef, useState } from 'react';

import type { Skin } from '@/api';
import { Empty } from '@/components/empty';
import { OfflineState } from '@/components/offline-state';
import { Page, Section } from '@/components/page';
import { SignInGate } from '@/components/sign-in-gate';
import { Button } from '@/components/ui/button';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { anchor, useAutoTour } from '@/features/onboarding';
import {
  CapeCard,
  CapeGrid,
  PreviewPanel,
  SkinCard,
  SkinGrid,
  SkinsPageSkeleton,
  skinDisplayName,
} from '@/features/skins/components';
import type { SkinDraft } from '@/features/skins/dialogs';
import { EditSkinDialog } from '@/features/skins/dialogs';
import { collapseDefaults, readTextureFile } from '@/features/skins/lib';
import { m } from '@/paraglide/messages.js';
import { useAccounts } from '@/queries';
import { useOffline } from '@/queries/net';
import { skinMutations, skinQueries } from '@/queries/skins';

const BASE64_PREFIX = /^data:image\/png;base64,/;

export function SkinsPage() {
  const { signedIn, isPending: accountsPending } = useAccounts();
  const offline = useOffline();
  const list = useQuery({ ...skinQueries.list(''), enabled: signedIn });
  useAutoTour('skins', signedIn && !list.isPending);

  const add = useMutation(skinMutations.add());
  const update = useMutation(skinMutations.update());
  const equip = useMutation(skinMutations.equip());
  const removeSkin = useMutation(skinMutations.remove());
  const equipCape = useMutation(skinMutations.equipCape());
  const clearCape = useMutation(skinMutations.clearCape());

  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [dialog, setDialog] = useState<{
    skin: Skin | null;
    texture?: string;
  } | null>(null);
  const [pendingRemove, setPendingRemove] = useState<Skin | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const skins = list.data?.skins ?? [];
  const capes = list.data?.capes ?? [];
  const saved = skins.filter((s) => s.source !== 'default');
  const defaults = collapseDefaults(skins);
  const shown = [...saved, ...defaults];
  const equipped = shown.find((s) => s.equipped);
  const equippedCape = capes.find((c) => c.equipped);
  // Resolve against the full wire list: the variant toggle can select a
  // default's sibling entry that the collapsed grid does not show.
  const selected =
    (selectedKey && skins.find((s) => s.key === selectedKey)) ||
    equipped ||
    shown[0];
  const isSelectedCard = (skin: Skin) =>
    selected != null &&
    (skin.key === selected.key ||
      (skin.source === 'default' &&
        selected.source === 'default' &&
        skin.name === selected.name));
  const previewing = selected != null && selected.key !== equipped?.key;

  const selectVariant = (variant: Skin['variant']) => {
    const sibling = skins.find(
      (s) =>
        s.source === 'default' &&
        s.name === selected?.name &&
        s.variant === variant,
    );
    if (sibling) setSelectedKey(sibling.key);
  };

  const applyCape = (capeId?: string) => {
    if (capeId === equippedCape?.id) return;
    if (capeId) equipCape.mutate({ cape: capeId });
    else clearCape.mutate(undefined);
  };

  const addFromFile = async (file: File | undefined) => {
    if (!file?.type.includes('png')) return;
    add.reset();
    setDialog({ skin: null, texture: await readTextureFile(file) });
  };

  const openEdit = (skin: Skin) => {
    update.reset();
    setDialog({ skin });
  };

  const saveDraft = (draft: SkinDraft) => {
    const editing = dialog?.skin;
    if (editing) {
      update.mutate(
        { key: editing.key, name: draft.name, variant: draft.variant },
        {
          onSuccess: () => {
            setDialog(null);
            applyCape(draft.capeId);
          },
        },
      );
    } else {
      add.mutate(
        {
          name: draft.name,
          variant: draft.variant,
          data: draft.texture.replace(BASE64_PREFIX, ''),
        },
        {
          onSuccess: (skin) => {
            setDialog(null);
            setSelectedKey(skin.key);
            applyCape(draft.capeId);
          },
        },
      );
    }
  };

  const body = !signedIn ? (
    <SignInGate
      title={m['skin.locked_title']()}
      hint={m['skin.sign_in_hint']()}
    />
  ) : offline && !list.data ? (
    <OfflineState />
  ) : list.isError && !list.data ? (
    <Empty icon={WarningCircleIcon} tone="destructive">
      {m['skin.load_failed']()}
    </Empty>
  ) : (
    <div className="flex items-start gap-6">
      {selected && (
        <span className="block" {...anchor('skin-preview')}>
          <PreviewPanel
            skin={selected}
            cape={equippedCape}
            previewing={previewing}
            onApply={() => equip.mutate({ key: selected.key })}
            onVariantChange={
              selected.source === 'default' ? selectVariant : undefined
            }
          />
        </span>
      )}

      <div className="min-w-0 flex-1 space-y-8" {...anchor('skin-library')}>
        <Section title={m['skin.your_skins']()} count={saved.length}>
          {saved.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              {m['skin.none_yet']()}
            </p>
          ) : (
            <SkinGrid>
              {saved.map((skin) => (
                <SkinCard
                  key={skin.key}
                  skin={skin}
                  selected={isSelectedCard(skin)}
                  equipped={skin.equipped}
                  onSelect={() => setSelectedKey(skin.key)}
                  onEquip={() => equip.mutate({ key: skin.key })}
                  onEdit={
                    skin.source !== 'default' ? () => openEdit(skin) : undefined
                  }
                  onRemove={
                    skin.source === 'library'
                      ? () => setPendingRemove(skin)
                      : undefined
                  }
                />
              ))}
            </SkinGrid>
          )}
        </Section>

        <Section title={m['skin.default_skins']()} count={defaults.length}>
          <SkinGrid>
            {defaults.map((skin) => (
              <SkinCard
                key={skin.key}
                skin={skin}
                selected={isSelectedCard(skin)}
                equipped={skin.equipped}
                onSelect={() => setSelectedKey(skin.key)}
                onEquip={() => equip.mutate({ key: skin.key })}
              />
            ))}
          </SkinGrid>
        </Section>

        <Section title={m['skin.capes']()} count={capes.length}>
          {capes.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              {m['skin.no_capes']()}
            </p>
          ) : (
            <CapeGrid>
              <CapeCard
                label={m['skin.no_cape']()}
                equipped={equippedCape == null}
                onEquip={() => clearCape.mutate(undefined)}
              />
              {capes.map((cape) => (
                <CapeCard
                  key={cape.id}
                  label={cape.name}
                  texture={cape.texture}
                  equipped={cape.equipped}
                  onEquip={() => equipCape.mutate({ cape: cape.id })}
                />
              ))}
            </CapeGrid>
          )}
        </Section>
      </div>
    </div>
  );

  return (
    <Page
      title={m['app.nav.skins']()}
      subtitle={m['skin.subtitle']()}
      skeleton={<SkinsPageSkeleton />}
      loading={accountsPending || (signedIn && list.isPending)}
      actions={
        <>
          <input
            ref={fileRef}
            type="file"
            accept="image/png"
            className="hidden"
            onChange={(e) => {
              addFromFile(e.target.files?.[0]);
              e.target.value = '';
            }}
          />
          <Button
            size="sm"
            data-icon="inline-start"
            disabled={!signedIn}
            className="bg-ember text-ember-foreground hover:bg-ember/90"
            onClick={() => fileRef.current?.click()}
          >
            <PlusIcon weight="bold" />
            {m['skin.add']()}
          </Button>
        </>
      }
    >
      {body}

      <EditSkinDialog
        open={dialog !== null}
        onOpenChange={(open) => {
          if (!open) setDialog(null);
        }}
        skin={dialog?.skin ?? null}
        initialTexture={dialog?.texture}
        capes={capes}
        equippedCapeId={equippedCape?.id}
        saving={add.isPending || update.isPending}
        onSave={saveDraft}
      />

      <ConfirmDialog
        open={pendingRemove !== null}
        onOpenChange={(open) => !open && setPendingRemove(null)}
        title={m['skin.delete_title']()}
        description={
          pendingRemove &&
          m['skin.delete_description']({
            name: skinDisplayName(pendingRemove),
          })
        }
        destructive
        confirmLabel={m['app.action.delete']()}
        onConfirm={() => {
          if (pendingRemove) removeSkin.mutate({ key: pendingRemove.key });
          setPendingRemove(null);
        }}
      />
    </Page>
  );
}
