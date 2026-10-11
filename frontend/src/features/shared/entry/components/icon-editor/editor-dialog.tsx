/**
 * The one place an icon is made: compose a generated one (background +
 * symbol), pick your own image, or reset to a fresh random one. The body sits
 * in the project's standard dialog chrome; the two-pane arrangement borrows
 * the Modrinth editor's shape without carrying its look. With an `entryId` the
 * choice bakes on save; the create wizard passes none and reads `onSaved`
 * instead.
 */
import {
  ArrowsClockwiseIcon,
  ImageIcon,
  PencilIcon,
  TrashIcon,
} from '@phosphor-icons/react';
import { useMutation, useQuery } from '@tanstack/react-query';
import { convertFileSrc } from '@tauri-apps/api/core';
import { useEffect, useMemo, useState } from 'react';

import { dialog } from '@/api';
import { type IconConfig, iconUrl } from '@/api/icons';
import { AppDialog, DialogFooter } from '@/components/dialog';
import { Button } from '@/components/ui/button';
import { Spinner } from '@/components/ui/spinner';
import { useDropTarget } from '@/hooks/use-drop-target';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';
import { iconMutations, iconQueries } from '@/queries/icons';

import {
  backgroundIdOf,
  backgroundOption,
  backgroundOptions,
  backgroundStyle,
  DEFAULT_BACKGROUND_ID,
  DEFAULT_SYMBOL_ID,
  randomIconConfig,
  type SymbolCategory,
  symbolBytes,
  symbolOption,
  symbolOptions,
} from './catalog';
import { IconOptionTile } from './swatch';

const symbolCategories: SymbolCategory[] = ['loader', 'modded', 'vanilla'];

function categoryLabel(category: SymbolCategory): string {
  switch (category) {
    case 'loader':
      return m['entry.icon.editor.categories.loader']();
    case 'modded':
      return m['entry.icon.editor.categories.modded']();
    case 'vanilla':
      return m['entry.icon.editor.categories.vanilla']();
  }
}

export function IconEditorDialog({
  open,
  onOpenChange,
  entryId,
  initial,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Present: bake/record for this entry on save; absent: only report. */
  entryId?: string;
  initial?: IconConfig | null;
  onSaved?: (config: IconConfig) => void;
}) {
  const [backgroundId, setBackgroundId] = useState(DEFAULT_BACKGROUND_ID);
  const [symbolId, setSymbolId] = useState(DEFAULT_SYMBOL_ID);
  const [customPath, setCustomPath] = useState<string | null>(null);
  const [customUrl, setCustomUrl] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const generate = useMutation(iconMutations.generate());
  const setIcon = useMutation(iconMutations.set());
  const icons = useQuery({
    ...iconQueries.list(),
    enabled: Boolean(entryId),
  });
  const storedConfig = useQuery({
    ...iconQueries.config(entryId ?? ''),
    enabled: Boolean(entryId),
  });

  useEffect(() => {
    if (!open) return;
    const fromStore = entryId ? storedConfig.data : null;
    const savedConfig = initial ?? fromStore;
    setBackgroundId(
      backgroundIdOf(savedConfig?.background) ?? DEFAULT_BACKGROUND_ID,
    );
    setSymbolId(
      savedConfig?.symbol && symbolOption(savedConfig.symbol)
        ? savedConfig.symbol
        : DEFAULT_SYMBOL_ID,
    );
    const existing = entryId ? icons.data?.[entryId] : undefined;
    const customEntry =
      entryId && existing && !fromStore ? existing : undefined;
    setCustomUrl(customEntry ? iconUrl(customEntry) : null);
    setCustomPath(customEntry?.path ?? null);
    setSaving(false);
  }, [open, entryId, initial, icons.data, storedConfig.data]);

  const selectedBackground = backgroundOption(backgroundId);
  const selectedSymbol = symbolOption(symbolId);
  const config = useMemo<IconConfig>(
    () => ({
      background: {
        ...(selectedBackground?.background ?? backgroundOptions[0].background),
      },
      symbol: symbolId,
    }),
    [selectedBackground, symbolId],
  );

  const applyCustomImage = (path: string) => {
    setCustomPath(path);
    setCustomUrl(convertFileSrc(path));
  };

  const pickCustomImage = async () => {
    const path = await dialog.pickImage();
    if (path) applyCustomImage(path);
  };

  // Only an existing entry can carry its own image; the create wizard has no
  // entry to record one against.
  const dropping = useDropTarget({
    extensions: dialog.IMAGE_EXTENSIONS,
    enabled: open && Boolean(entryId),
    onDrop: ([image]) => applyCustomImage(image),
  });

  const applyRandom = () => {
    const rolled = randomIconConfig(config);
    const background = backgroundIdOf(rolled.background);
    if (background) setBackgroundId(background);
    setSymbolId(rolled.symbol);
    setCustomPath(null);
    setCustomUrl(null);
    return rolled;
  };

  const saveIcon = async () => {
    if (saving) return;
    setSaving(true);
    try {
      if (customUrl) {
        if (entryId && customPath)
          await setIcon.mutateAsync({ entryId, sourcePath: customPath });
      } else if (selectedSymbol) {
        const bytes = await symbolBytes(selectedSymbol.asset);
        if (entryId)
          await generate.mutateAsync({ entryId, config, symbolBytes: bytes });
        else onSaved?.(config);
      }
      onOpenChange(false);
    } finally {
      setSaving(false);
    }
  };

  const resetIcon = async () => {
    if (saving || !entryId) return;
    setSaving(true);
    try {
      const rolled = applyRandom();
      const symbol = symbolOption(rolled.symbol);
      if (!symbol) return;
      await generate.mutateAsync({
        entryId,
        config: rolled,
        symbolBytes: await symbolBytes(symbol.asset),
      });
      onOpenChange(false);
    } finally {
      setSaving(false);
    }
  };

  return (
    <AppDialog
      open={open}
      onOpenChange={onOpenChange}
      size="2xl"
      icon={PencilIcon}
      title={m['entry.icon.editor.title']()}
      description={m['entry.icon.editor.description']()}
    >
      <div className="flex max-h-[30rem] min-h-0 flex-1 gap-4 overflow-hidden">
        <aside className="flex w-44 shrink-0 flex-col gap-3 overflow-y-auto">
          <IconPreview
            config={config}
            customUrl={customUrl}
            className={cn(
              'size-32 self-center',
              dropping && 'outline-2 outline-primary outline-dashed',
            )}
          />

          <Button
            type="button"
            variant="outline"
            className="w-full"
            data-icon="inline-start"
            onClick={applyRandom}
          >
            <ArrowsClockwiseIcon />
            {m['entry.icon.editor.randomize']()}
          </Button>

          {entryId && (
            <>
              <div className="flex flex-col gap-2">
                <span className="text-xs font-medium">
                  {m['entry.icon.editor.image']()}
                </span>
                <div className="flex items-center gap-2">
                  {customUrl && (
                    <img
                      src={customUrl}
                      alt=""
                      className="size-8 shrink-0 object-cover ring-1 ring-border"
                    />
                  )}
                  <Button
                    type="button"
                    variant="outline"
                    className="flex-1"
                    data-icon="inline-start"
                    onClick={pickCustomImage}
                  >
                    <ImageIcon />
                    {m['entry.icon.editor.pick_image']()}
                  </Button>
                </div>
              </div>

              <Button
                type="button"
                variant="outline"
                className="w-full"
                data-icon="inline-start"
                onClick={resetIcon}
                disabled={saving}
              >
                <TrashIcon />
                {m['entry.icon.reset']()}
              </Button>
            </>
          )}
        </aside>

        <div className="min-w-0 flex-1 overflow-y-auto">
          <section className="mb-6">
            <h3 className="mb-2 text-xs font-medium">
              {m['entry.icon.editor.background']()}
            </h3>
            <div className="grid grid-cols-6 gap-2">
              {backgroundOptions.map((option) => (
                <IconOptionTile
                  key={option.id}
                  selected={backgroundId === option.id}
                  onSelect={() => {
                    setCustomPath(null);
                    setCustomUrl(null);
                    setBackgroundId(option.id);
                  }}
                  label={option.name()}
                  style={backgroundStyle(option.background)}
                />
              ))}
            </div>
          </section>

          <section>
            <h3 className="mb-2 text-xs font-medium">
              {m['entry.icon.editor.symbol']()}
            </h3>
            <div className="flex flex-col gap-4">
              {symbolCategories.map((category) => (
                <div key={category}>
                  <p className="mb-2 text-[10px] font-semibold tracking-wide text-muted-foreground uppercase">
                    {categoryLabel(category)}
                  </p>
                  <div className="grid grid-cols-6 gap-2">
                    {symbolOptions
                      .filter((option) => option.category === category)
                      .map((option) => (
                        <IconOptionTile
                          key={option.id}
                          selected={symbolId === option.id}
                          onSelect={() => {
                            setCustomPath(null);
                            setCustomUrl(null);
                            setSymbolId(option.id);
                          }}
                          label={option.name()}
                          className="bg-muted/50"
                        >
                          <img
                            src={option.asset}
                            alt=""
                            className="size-full object-cover"
                          />
                        </IconOptionTile>
                      ))}
                  </div>
                </div>
              ))}
            </div>
          </section>
        </div>
      </div>

      <DialogFooter>
        <Button
          type="button"
          variant="outline"
          onClick={() => onOpenChange(false)}
          disabled={saving}
        >
          {m['app.action.cancel']()}
        </Button>
        <Button
          type="button"
          className="bg-ember text-ember-foreground hover:bg-ember/90"
          onClick={saveIcon}
          disabled={saving}
        >
          {saving ? <Spinner /> : null}
          {m['entry.icon.editor.save']()}
        </Button>
      </DialogFooter>
    </AppDialog>
  );
}

/** The current choice at one size: a picked image or the composed background + symbol. */
function IconPreview({
  config,
  customUrl,
  className,
}: {
  config: IconConfig;
  customUrl: string | null;
  className?: string;
}) {
  const symbol = symbolOption(config.symbol);
  return (
    <div
      role="img"
      aria-label={previewName(config, customUrl)}
      className={cn(
        'relative aspect-square shrink-0 overflow-hidden ring-1 ring-border',
        className,
      )}
      style={customUrl ? undefined : backgroundStyle(config.background)}
    >
      {customUrl ? (
        <img src={customUrl} alt="" className="size-full object-cover" />
      ) : (
        symbol && (
          <img src={symbol.asset} alt="" className="size-full object-cover" />
        )
      )}
    </div>
  );
}

/** "Background · Symbol", or "Custom image" when a picked image takes over. */
function previewName(config: IconConfig, customUrl: string | null): string {
  if (customUrl) return m['entry.icon.editor.image']();
  const background = backgroundOption(
    backgroundIdOf(config.background) ?? '',
  )?.name();
  const symbol = symbolOption(config.symbol)?.name() ?? config.symbol;
  return [background, symbol].filter(Boolean).join(' · ');
}
