/**
 * The generated-icon catalogue: the curated sprite set (GPL-3.0, vendor
 * logos and club-of-this-launcher sprites excluded), the gradient palette,
 * and the pieces the editor and the wizard both build on — ids, configs,
 * the randomization blacklist and the canvas baker.
 *
 * The background and symbol ids are the literal sidecar values the editor
 * reopens from; keep them stable or stored icons break.
 */

import type { CSSProperties } from 'react';
import type { IconBackground, IconConfig } from '@/api/icons';
import backpack from '@/assets/icons/backpack.png';
import beacon from '@/assets/icons/beacon.png';
import blueShark from '@/assets/icons/blue-shark.png';
import bookshelf from '@/assets/icons/bookshelf.png';
import brownBear from '@/assets/icons/brown-bear.png';
import cake from '@/assets/icons/cake.png';
import campfire from '@/assets/icons/campfire.png';
import chest from '@/assets/icons/chest.png';
import cogwheel from '@/assets/icons/cogwheel.png';
import commandBlock from '@/assets/icons/command-block.png';
import cookingPot from '@/assets/icons/cooking-pot.png';
import couch from '@/assets/icons/couch.png';
import craftingTable from '@/assets/icons/crafting-table.png';
import creeper from '@/assets/icons/creeper.png';
import enchantingTable from '@/assets/icons/enchanting-table.png';
import enderChest from '@/assets/icons/ender-chest.png';
import enderDragon from '@/assets/icons/ender-dragon.png';
import engine from '@/assets/icons/engine.png';
import fabric from '@/assets/icons/fabric.png';
import forge from '@/assets/icons/forge.png';
import furnace from '@/assets/icons/furnace.png';
import gizmo from '@/assets/icons/gizmo.png';
import globe from '@/assets/icons/globe.png';
import grassBlock from '@/assets/icons/grass-block.png';
import lantern from '@/assets/icons/lantern.png';
import moobloom from '@/assets/icons/moobloom.png';
import neoforge from '@/assets/icons/neoforge.png';
import orb from '@/assets/icons/orb.png';
import oxygenDistributor from '@/assets/icons/oxygen-distributor.png';
import pancakes from '@/assets/icons/pancakes.png';
import pickaxe from '@/assets/icons/pickaxe.png';
import pokeBall from '@/assets/icons/poke-ball.png';
import quilt from '@/assets/icons/quilt.png';
import redstoneBlock from '@/assets/icons/redstone-block.png';
import sculkSensor from '@/assets/icons/sculk-sensor.png';
import skeleton from '@/assets/icons/skeleton.png';
import skillet from '@/assets/icons/skillet.png';
import slimeBlock from '@/assets/icons/slime-block.png';
import spaceHelmet from '@/assets/icons/space-helmet.png';
import stickyPiston from '@/assets/icons/sticky-piston.png';
import sword from '@/assets/icons/sword.png';
import terminal from '@/assets/icons/terminal.png';
import tinyPotato from '@/assets/icons/tiny-potato.png';
import tire from '@/assets/icons/tire.png';
import tnt from '@/assets/icons/tnt.png';
import wrench from '@/assets/icons/wrench.png';
import zombie from '@/assets/icons/zombie.png';
import { m } from '@/paraglide/messages.js';

export type SymbolCategory = 'loader' | 'modded' | 'vanilla';

interface BackgroundOption {
  id: string;
  name: () => string;
  background: Extract<
    IconConfig['background'],
    { type: 'linear-top-down-gradient' }
  >;
}

interface SymbolOption {
  id: string;
  name: () => string;
  asset: string;
  category: SymbolCategory;
  excludeFromRandomization?: boolean;
}

export const backgroundOptions: BackgroundOption[] = [
  {
    id: 'rose',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#D62E63',
      bottom_color: '#F95C62',
    },
    name: () => m['entry.icon.editor.backgrounds.rose'](),
  },
  {
    id: 'orange',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#FF8D29',
      bottom_color: '#FFB452',
    },
    name: () => m['entry.icon.editor.backgrounds.orange'](),
  },
  {
    id: 'yellow',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#FFC629',
      bottom_color: '#FFEE53',
    },
    name: () => m['entry.icon.editor.backgrounds.yellow'](),
  },
  {
    id: 'lime',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#6FDA1D',
      bottom_color: '#CBFF50',
    },
    name: () => m['entry.icon.editor.backgrounds.lime'](),
  },
  {
    id: 'green',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#0B9F21',
      bottom_color: '#4FD24B',
    },
    name: () => m['entry.icon.editor.backgrounds.green'](),
  },
  {
    id: 'purple',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#4739FF',
      bottom_color: '#6670FF',
    },
    name: () => m['entry.icon.editor.backgrounds.purple'](),
  },
  {
    id: 'blue',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#227EFF',
      bottom_color: '#5EC1FF',
    },
    name: () => m['entry.icon.editor.backgrounds.blue'](),
  },
  {
    id: 'lavender',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#C056FD',
      bottom_color: '#B889FF',
    },
    name: () => m['entry.icon.editor.backgrounds.lavender'](),
  },
  {
    id: 'pink',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#F640C0',
      bottom_color: '#FF7BF1',
    },
    name: () => m['entry.icon.editor.backgrounds.pink'](),
  },
  {
    id: 'light_gray',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#AEAEAE',
      bottom_color: '#D9D9D9',
    },
    name: () => m['entry.icon.editor.backgrounds.light_gray'](),
  },
  {
    id: 'gray',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#373C4C',
      bottom_color: '#4C4F58',
    },
    name: () => m['entry.icon.editor.backgrounds.gray'](),
  },
  {
    id: 'dark_gray',
    background: {
      type: 'linear-top-down-gradient',
      top_color: '#1B1D29',
      bottom_color: '#252731',
    },
    name: () => m['entry.icon.editor.backgrounds.dark_gray'](),
  },
];

export const symbolOptions: SymbolOption[] = [
  {
    id: 'fabric',
    name: () => m['entry.icon.editor.symbols.fabric'](),
    asset: fabric,
    category: 'loader',
    excludeFromRandomization: true,
  },
  {
    id: 'forge',
    name: () => m['entry.icon.editor.symbols.forge'](),
    asset: forge,
    category: 'loader',
    excludeFromRandomization: true,
  },
  {
    id: 'neoforge',
    name: () => m['entry.icon.editor.symbols.neoforge'](),
    asset: neoforge,
    category: 'loader',
    excludeFromRandomization: true,
  },
  {
    id: 'quilt',
    name: () => m['entry.icon.editor.symbols.quilt'](),
    asset: quilt,
    category: 'loader',
    excludeFromRandomization: true,
  },

  {
    id: 'poke_ball',
    name: () => m['entry.icon.editor.symbols.poke_ball'](),
    asset: pokeBall,
    category: 'modded',
  },
  {
    id: 'orb',
    name: () => m['entry.icon.editor.symbols.orb'](),
    asset: orb,
    category: 'modded',
  },
  {
    id: 'cooking_pot',
    name: () => m['entry.icon.editor.symbols.cooking_pot'](),
    asset: cookingPot,
    category: 'modded',
  },
  {
    id: 'skillet',
    name: () => m['entry.icon.editor.symbols.skillet'](),
    asset: skillet,
    category: 'modded',
  },
  {
    id: 'globe',
    name: () => m['entry.icon.editor.symbols.globe'](),
    asset: globe,
    category: 'modded',
  },
  {
    id: 'pancakes',
    name: () => m['entry.icon.editor.symbols.pancakes'](),
    asset: pancakes,
    category: 'modded',
  },
  {
    id: 'backpack',
    name: () => m['entry.icon.editor.symbols.backpack'](),
    asset: backpack,
    category: 'modded',
  },
  {
    id: 'couch',
    name: () => m['entry.icon.editor.symbols.couch'](),
    asset: couch,
    category: 'modded',
  },
  {
    id: 'tiny_potato',
    name: () => m['entry.icon.editor.symbols.tiny_potato'](),
    asset: tinyPotato,
    category: 'modded',
  },
  {
    id: 'blue_shark',
    name: () => m['entry.icon.editor.symbols.blue_shark'](),
    asset: blueShark,
    category: 'modded',
  },
  {
    id: 'brown_bear',
    name: () => m['entry.icon.editor.symbols.brown_bear'](),
    asset: brownBear,
    category: 'modded',
  },
  {
    id: 'moobloom',
    name: () => m['entry.icon.editor.symbols.moobloom'](),
    asset: moobloom,
    category: 'modded',
  },
  {
    id: 'create_wrench',
    name: () => m['entry.icon.editor.symbols.create_wrench'](),
    asset: wrench,
    category: 'modded',
  },
  {
    id: 'cogwheel',
    name: () => m['entry.icon.editor.symbols.cogwheel'](),
    asset: cogwheel,
    category: 'modded',
  },
  {
    id: 'engine',
    name: () => m['entry.icon.editor.symbols.engine'](),
    asset: engine,
    category: 'modded',
  },
  {
    id: 'tire',
    name: () => m['entry.icon.editor.symbols.tire'](),
    asset: tire,
    category: 'modded',
  },
  {
    id: 'oxygen_distributor',
    name: () => m['entry.icon.editor.symbols.oxygen_distributor'](),
    asset: oxygenDistributor,
    category: 'modded',
  },
  {
    id: 'space_helmet',
    name: () => m['entry.icon.editor.symbols.space_helmet'](),
    asset: spaceHelmet,
    category: 'modded',
  },
  {
    id: 'gizmo',
    name: () => m['entry.icon.editor.symbols.gizmo'](),
    asset: gizmo,
    category: 'modded',
  },
  {
    id: 'terminal',
    name: () => m['entry.icon.editor.symbols.terminal'](),
    asset: terminal,
    category: 'modded',
  },

  {
    id: 'grass_block',
    name: () => m['entry.icon.editor.symbols.grass_block'](),
    asset: grassBlock,
    category: 'vanilla',
  },
  {
    id: 'crafting_table',
    name: () => m['entry.icon.editor.symbols.crafting_table'](),
    asset: craftingTable,
    category: 'vanilla',
  },
  {
    id: 'furnace',
    name: () => m['entry.icon.editor.symbols.furnace'](),
    asset: furnace,
    category: 'vanilla',
  },
  {
    id: 'chest',
    name: () => m['entry.icon.editor.symbols.chest'](),
    asset: chest,
    category: 'vanilla',
  },
  {
    id: 'bookshelf',
    name: () => m['entry.icon.editor.symbols.bookshelf'](),
    asset: bookshelf,
    category: 'vanilla',
  },
  {
    id: 'redstone_block',
    name: () => m['entry.icon.editor.symbols.redstone_block'](),
    asset: redstoneBlock,
    category: 'vanilla',
  },
  {
    id: 'sticky_piston',
    name: () => m['entry.icon.editor.symbols.sticky_piston'](),
    asset: stickyPiston,
    category: 'vanilla',
  },
  {
    id: 'slime_block',
    name: () => m['entry.icon.editor.symbols.slime_block'](),
    asset: slimeBlock,
    category: 'vanilla',
  },
  {
    id: 'cake',
    name: () => m['entry.icon.editor.symbols.cake'](),
    asset: cake,
    category: 'vanilla',
  },
  {
    id: 'campfire',
    name: () => m['entry.icon.editor.symbols.campfire'](),
    asset: campfire,
    category: 'vanilla',
  },
  {
    id: 'pickaxe',
    name: () => m['entry.icon.editor.symbols.pickaxe'](),
    asset: pickaxe,
    category: 'vanilla',
  },
  {
    id: 'sword',
    name: () => m['entry.icon.editor.symbols.sword'](),
    asset: sword,
    category: 'vanilla',
  },
  {
    id: 'zombie',
    name: () => m['entry.icon.editor.symbols.zombie'](),
    asset: zombie,
    category: 'vanilla',
  },
  {
    id: 'creeper',
    name: () => m['entry.icon.editor.symbols.creeper'](),
    asset: creeper,
    category: 'vanilla',
  },
  {
    id: 'skeleton',
    name: () => m['entry.icon.editor.symbols.skeleton'](),
    asset: skeleton,
    category: 'vanilla',
  },
  {
    id: 'ender_dragon',
    name: () => m['entry.icon.editor.symbols.ender_dragon'](),
    asset: enderDragon,
    category: 'vanilla',
  },
  {
    id: 'ender_chest',
    name: () => m['entry.icon.editor.symbols.ender_chest'](),
    asset: enderChest,
    category: 'vanilla',
  },
  {
    id: 'sculk_sensor',
    name: () => m['entry.icon.editor.symbols.sculk_sensor'](),
    asset: sculkSensor,
    category: 'vanilla',
  },
  {
    id: 'beacon',
    name: () => m['entry.icon.editor.symbols.beacon'](),
    asset: beacon,
    category: 'vanilla',
  },
  {
    id: 'enchanting_table',
    name: () => m['entry.icon.editor.symbols.enchanting_table'](),
    asset: enchantingTable,
    category: 'vanilla',
  },
  {
    id: 'lantern',
    name: () => m['entry.icon.editor.symbols.lantern'](),
    asset: lantern,
    category: 'vanilla',
  },
  {
    id: 'tnt',
    name: () => m['entry.icon.editor.symbols.tnt'](),
    asset: tnt,
    category: 'vanilla',
  },
  {
    id: 'command_block',
    name: () => m['entry.icon.editor.symbols.command_block'](),
    asset: commandBlock,
    category: 'vanilla',
  },
];

export const DEFAULT_BACKGROUND_ID = 'purple';
export const DEFAULT_SYMBOL_ID = 'grass_block';

export function backgroundOption(id: string): BackgroundOption | undefined {
  return backgroundOptions.find((option) => option.id === id);
}

export function backgroundIdOf(
  background: IconConfig['background'] | undefined,
): string | undefined {
  if (!background || background.type === 'color') return undefined;
  return backgroundOptions.find(
    (option) =>
      option.background.top_color === background.top_color &&
      option.background.bottom_color === background.bottom_color,
  )?.id;
}

export function symbolOption(id: string): SymbolOption | undefined {
  return symbolOptions.find((option) => option.id === id);
}

/** The CSS fill that previews a wire background: a flat colour or a gradient. */
export function backgroundStyle(background: IconBackground): CSSProperties {
  if (background.type === 'color') return { backgroundColor: background.value };
  return {
    backgroundImage: `linear-gradient(to bottom, ${background.top_color}, ${background.bottom_color})`,
  };
}

const RANDOM_BLACKLIST: Array<{ background: string; symbol: string }> = [
  { background: 'purple', symbol: 'globe' },
  { background: 'blue', symbol: 'globe' },
  { background: 'gray', symbol: 'cogwheel' },
  { background: 'dark_gray', symbol: 'cogwheel' },
  { background: 'rose', symbol: 'poke_ball' },
  { background: 'lime', symbol: 'slime_block' },
  { background: 'green', symbol: 'slime_block' },
  { background: 'rose', symbol: 'redstone_block' },
  { background: 'rose', symbol: 'couch' },
  { background: 'orange', symbol: 'space_helmet' },
  { background: 'rose', symbol: 'tnt' },
  { background: 'yellow', symbol: 'moobloom' },
  { background: 'light_gray', symbol: 'skillet' },
  { background: 'light_gray', symbol: 'cooking_pot' },
];

function isBlocked(backgroundId: string, symbolId: string): boolean {
  return RANDOM_BLACKLIST.some(
    (blocked) =>
      blocked.background === backgroundId && blocked.symbol === symbolId,
  );
}

/** Roll a config distinct from `current`, avoiding the look-alike clashes. */
export function randomIconConfig(current?: IconConfig): IconConfig {
  const currentBackground = current
    ? backgroundIdOf(current.background)
    : undefined;
  const candidates = backgroundOptions.flatMap((background) =>
    symbolOptions
      .filter(
        (symbol) =>
          !symbol.excludeFromRandomization &&
          background.id !== currentBackground &&
          symbol.id !== current?.symbol &&
          !isBlocked(background.id, symbol.id),
      )
      .map((symbol) => ({ background: background.id, symbol: symbol.id })),
  );
  if (candidates.length === 0) {
    return {
      background: { ...backgroundOptions[0].background },
      symbol: DEFAULT_SYMBOL_ID,
    };
  }
  const pick = candidates[Math.floor(Math.random() * candidates.length)];
  const chosenBackground = backgroundOption(pick.background);
  return {
    background: {
      ...(chosenBackground?.background ?? backgroundOptions[0].background),
    },
    symbol: pick.symbol,
  };
}

const BAKED_ICON_SIZE = 256;

function backgroundFill(
  context: CanvasRenderingContext2D,
  background: IconBackground,
): string | CanvasGradient {
  if (background.type === 'color') return background.value;
  const gradient = context.createLinearGradient(0, 0, 0, BAKED_ICON_SIZE);
  gradient.addColorStop(0, background.top_color);
  gradient.addColorStop(1, background.bottom_color);
  return gradient;
}

/** Paint the config onto a 256px canvas and encode it as opaque PNG bytes; assets are same-origin. */
export async function bakeIcon(config: IconConfig): Promise<number[]> {
  const symbol = symbolOption(config.symbol);
  if (!symbol) throw new Error(`Unknown icon symbol: ${config.symbol}`);
  const image = new Image();
  image.src = symbol.asset;
  await image.decode();

  const canvas = document.createElement('canvas');
  canvas.width = BAKED_ICON_SIZE;
  canvas.height = BAKED_ICON_SIZE;
  const context = canvas.getContext('2d');
  if (!context) throw new Error('Canvas is unavailable');
  context.fillStyle = backgroundFill(context, config.background);
  context.fillRect(0, 0, BAKED_ICON_SIZE, BAKED_ICON_SIZE);
  context.drawImage(image, 0, 0, BAKED_ICON_SIZE, BAKED_ICON_SIZE);

  const blob = await new Promise<Blob | null>((resolve) =>
    canvas.toBlob(resolve, 'image/png'),
  );
  if (!blob) throw new Error('Failed to encode the icon');
  return Array.from(new Uint8Array(await blob.arrayBuffer()));
}
