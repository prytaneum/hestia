/**
 * The `skin.*` / `cape.*` channels: the account's Mojang skin picture (saved
 * library + vanilla defaults + owned capes) and the operations over it.
 * `account` is a name or uuid everywhere; empty uses the default account.
 */
import { call } from './core/ipc';
import type { Skin, SkinListResult, SkinVariant } from './types/skins';

export function list(account = ''): Promise<SkinListResult> {
  return call('skin.list', { account });
}

/**
 * Upload a skin PNG, equip it, and save it to the library. `data` is the PNG
 * base64-encoded (64×64, or the legacy 64×32).
 */
export async function add(params: {
  account?: string;
  name?: string;
  variant: SkinVariant;
  data: string;
}): Promise<Skin> {
  const result = await call('skin.add', { account: '', name: '', ...params });
  return result.skin;
}

/**
 * Rewrite a library entry's label and variant. A variant change on the
 * equipped skin is re-pushed to Mojang; `list` stays the authority on which
 * skin is equipped.
 */
export async function update(params: {
  account?: string;
  key: string;
  name: string;
  variant: SkinVariant;
}): Promise<Skin> {
  const result = await call('skin.update', { account: '', ...params });
  return result.skin;
}

/** Equip a library or default skin by its key from `list`. */
export async function equip(key: string, account = ''): Promise<void> {
  await call('skin.equip', { account, key });
}

/** Remove a library entry; the equipped Mojang skin is untouched. */
export async function remove(key: string): Promise<void> {
  await call('skin.remove', { key });
}

export async function equipCape(cape: string, account = ''): Promise<void> {
  await call('cape.equip', { account, cape });
}

export async function clearCape(account = ''): Promise<void> {
  await call('cape.clear', { account });
}
