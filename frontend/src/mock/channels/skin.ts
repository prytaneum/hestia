/**
 * `skin.*` / `cape.*` — the account's skin picture: the saved library, the
 * vanilla defaults, and the capes it owns. At most one of each is equipped,
 * which the list is the authority on.
 *
 * The defaults mirror `crates/engine/src/skins/defaults.rs` — the nine 1.19.3+
 * characters in both model variants, keyed by their Mojang texture hash.
 */
import type { Cape, Skin, SkinVariant } from '@/api/types';

import { fail, type Handlers, ok, str } from '../support';
import { capeTexture, defaultTextures, libraryTexture } from './textures';

const CHARACTERS: [name: string, slim: string, classic: string][] = [
  [
    'Steve',
    'd5c4ee5ce20aed9e33e866c66caa37178606234b3721084bf01d13320fb2eb3f',
    '31f477eb1a7beee631c2ca64d06f8f68fa93a3386d04452ab27f43acdf1b60cb',
  ],
  [
    'Alex',
    '46acd06e8483b176e8ea39fc12fe105eb3a2a4970f5100057e9d84d4b60bdfa7',
    '1abc803022d8300ab7578b189294cce39622d9a404cdc00d3feacfdf45be6981',
  ],
  [
    'Ari',
    '6ac6ca262d67bcfb3dbc924ba8215a18195497c780058a5749de674217721892',
    '4c05ab9e07b3505dc3ec11370c3bdce5570ad2fb2b562e9b9dd9cf271f81aa44',
  ],
  [
    'Efe',
    'fece7017b1bb13926d1158864b283b8b930271f80a90482f174cca6a17e88236',
    'daf3d88ccb38f11f74814e92053d92f7728ddb1a7955652a60e30cb27ae6659f',
  ],
  [
    'Kai',
    '226c617fde5b1ba569aa08bd2cb6fd84c93337532a872b3eb7bf66bdd5b395f8',
    'e5cdc3243b2153ab28a159861be643a4fc1e3c17d291cdd3e57a7f370ad676f3',
  ],
  [
    'Makena',
    '7cb3ba52ddd5cc82c0b050c3f920f87da36add80165846f479079663805433db',
    'dc0fcfaf2aa040a83dc0de4e56058d1bbb2ea40157501f3e7d15dc245e493095',
  ],
  [
    'Noor',
    '6c160fbd16adbc4bff2409e70180d911002aebcfa811eb6ec3d1040761aea6dd',
    '90e75cd429ba6331cd210b9bd19399527ee3bab467b5a9f61cb8a27b177f6789',
  ],
  [
    'Sunny',
    'b66bc80f002b10371e2fa23de6f230dd5e2f3affc2e15786f65bc9be4c6eb71a',
    'a3bd16079f764cd541e072e888fe43885e711f98658323db0f9a6045da91ee7a',
  ],
  [
    'Zuri',
    'eee522611005acf256dbd152e992c60c0bb7978cb0f3127807700e478ad97664',
    'f5dddb41dcafef616e959c2817808e0be741c89ffbfed39134a13e75b811863d',
  ],
];

function asDefault(name: string, key: string, variant: SkinVariant): Skin {
  return {
    key,
    name,
    variant,
    texture: defaultTextures[key],
    source: 'default',
    equipped: false,
  };
}

const defaults: Skin[] = CHARACTERS.flatMap(([name, slim, classic]) => [
  asDefault(name, slim, 'slim'),
  asDefault(name, classic, 'classic'),
]);

const library: Skin[] = [
  {
    key: 'library-ember',
    name: 'Ember',
    variant: 'classic',
    texture: libraryTexture,
    source: 'library',
    equipped: true,
  },
];

const capes: Cape[] = [
  {
    id: 'migrator',
    name: 'Migrator',
    texture: capeTexture,
    equipped: false,
  },
];

const all = (): Skin[] => [...library, ...defaults];

function equip(key: string): void {
  for (const skin of all()) skin.equipped = skin.key === key;
}

export const channels: Handlers = {
  'skin.list': () => ({ skins: all(), capes }),

  'skin.add': (p) => {
    const skin: Skin = {
      key: `library-${library.length + 1}`,
      name: str(p, 'name', 'New Skin'),
      variant: str(p, 'variant', 'classic') === 'slim' ? 'slim' : 'classic',
      // The upload is base64 PNG on the wire; the library serves it back as
      // the data URL the preview renders from.
      texture: `data:image/png;base64,${str(p, 'data')}`,
      source: 'library',
      equipped: true,
    };
    library.unshift(skin);
    equip(skin.key);
    return { skin };
  },

  'skin.update': (p) => {
    const key = str(p, 'key');
    const skin = library.find((entry) => entry.key === key);
    if (!skin) fail('not_found', `no such skin: ${key}`);
    skin.name = str(p, 'name', skin.name);
    skin.variant =
      str(p, 'variant', skin.variant) === 'slim' ? 'slim' : 'classic';
    return { skin };
  },

  'skin.equip': (p) => {
    equip(str(p, 'key'));
    return ok();
  },

  'skin.remove': (p) => {
    const at = library.findIndex((entry) => entry.key === str(p, 'key'));
    if (at >= 0) library.splice(at, 1);
    return ok();
  },

  'cape.equip': (p) => {
    const id = str(p, 'cape');
    for (const cape of capes) cape.equipped = cape.id === id;
    return ok();
  },

  'cape.clear': () => {
    for (const cape of capes) cape.equipped = false;
    return ok();
  },
};
