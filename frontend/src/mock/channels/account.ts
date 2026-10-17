/**
 * `account.*`. The desktop signs in over the shell's `account_login_sisu`
 * command (see ../commands/auth), which drives the two channels below around
 * a native webview; they are here because the device-code flow shares them.
 */
import type { Account, AccountListResult } from '@/api/types';

import { type ChannelHandlers, fail, str } from '../support';

export const player: Account = {
  uuid: '00000000-0000-0000-0000-000000000001',
  name: 'Player',
  needsReauth: false,
};

const accounts: Account[] = [
  player,
  {
    uuid: '00000000-0000-0000-0000-000000000002',
    name: 'AltPlayer',
    needsReauth: true,
  },
];

let defaultUuid = player.uuid;

function find(ref: string): Account {
  const found = accounts.find(
    (account) => account.uuid === ref || account.name === ref,
  );
  if (!found) fail('not_found', `no such account: ${ref}`);
  return found;
}

/** The signed-in account a launch runs as; empty `ref` means the default. */
export const resolve = (ref = ''): Account =>
  ref ? find(ref) : (accounts.find((a) => a.uuid === defaultUuid) ?? player);

export const channels: ChannelHandlers = {
  'account.list': (): AccountListResult => ({ accounts, defaultUuid }),

  'account.login.begin': (p) => ({
    id: 'login-1',
    method: str(p, 'method') === 'device_code' ? 'device_code' : 'sisu',
    url: 'https://login.live.com/oauth20_authorize.srf',
    userCode: 'MOCK-CODE',
    verificationUri: 'https://microsoft.com/link',
  }),

  'account.login.complete': () => player,

  'account.switch': (p) => {
    const account = find(str(p, 'account'));
    defaultUuid = account.uuid;
    return account;
  },

  'account.remove': (p) => {
    const account = find(str(p, 'account'));
    accounts.splice(accounts.indexOf(account), 1);
    if (defaultUuid === account.uuid) defaultUuid = accounts[0]?.uuid ?? '';
    return {};
  },
};
