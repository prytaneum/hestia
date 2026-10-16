/**
 * `account.*` — queries/mutations plus their 1:1 hooks. Sign-in is the
 * two-step flow: `useBeginLogin` yields what the user must act on (the sisu
 * URL, or a device code), `useCompleteLogin` blocks until the account is
 * stored — hence its long-lived pending state.
 */
import type { QueryClient } from '@tanstack/react-query';
import {
  queryOptions,
  useMutation,
  useMutationState,
  useQuery,
} from '@tanstack/react-query';
import type { Account, AccountLoginBeginResult, LoginMethod } from '../api';
import * as api from '../api/accounts';
import { mutation } from './core';
import { keys } from './keys';

const accountQueries = {
  list: () =>
    queryOptions({
      queryKey: keys.accounts.list(),
      queryFn: () => api.list(),
    }),
};

/**
 * The daemon refuses `instance.*` and `sync.*` until an account is signed in
 * (`daemon/src/runtime/router.rs::requires_account`), and an errored query is
 * not retried on mount — so anything read behind the gate stays stuck on its
 * `unauthorized` failure. Every mutation that opens or closes the gate sweeps
 * them along with its own keys.
 */
const gateKeys = () => [
  keys.accounts.all,
  keys.skins.all,
  keys.instances.all,
  keys.sync.all,
];

const accountMutations = {
  /**
   * The desktop sign-in: one shell command drives the whole sisu flow behind a
   * native Microsoft window. Resolves to the new account, or `null` on cancel.
   */
  loginSisu: () =>
    mutation<Account | null, void>({
      mutationKey: [...keys.accounts.all, 'login', 'sisu'],
      mutationFn: () => api.loginSisu(),
      invalidates: gateKeys,
    }),
  beginLogin: () =>
    mutation<AccountLoginBeginResult, LoginMethod>({
      mutationKey: [...keys.accounts.all, 'login', 'begin'],
      mutationFn: (method) => api.beginLogin(method),
    }),
  completeLogin: () =>
    mutation<Account, { id: string; code?: string }>({
      mutationKey: [...keys.accounts.all, 'login', 'complete'],
      mutationFn: ({ id, code }) => api.completeLogin(id, code),
      invalidates: gateKeys,
    }),
  /** Pick the default account launches use; `account` is a name or uuid. */
  switch: () =>
    mutation<Account, string>({
      mutationKey: [...keys.accounts.all, 'switch'],
      mutationFn: (account) => api.switchAccount(account),
      invalidates: () => [keys.accounts.all, keys.skins.all],
    }),
  remove: () =>
    mutation<void, string>({
      mutationKey: [...keys.accounts.all, 'remove'],
      mutationFn: (account) => api.remove(account),
      invalidates: gateKeys,
    }),
};

export function useAccounts() {
  const query = useQuery(accountQueries.list());
  const login = useMutation(accountMutations.loginSisu());
  const beginLogin = useMutation(accountMutations.beginLogin());
  const completeLogin = useMutation(accountMutations.completeLogin());
  const switchAccount = useMutation(accountMutations.switch());
  const remove = useMutation(accountMutations.remove());

  // Every surface that offers sign-in mounts its own observer, so a login
  // started from one leaves the others' `isPending`/`isError` false. The
  // shared truth is the mutation cache: read the state of every login under
  // the key, wherever it was fired from.
  const logins = useMutationState({
    filters: { mutationKey: [...keys.accounts.all, 'login'] },
    select: (m) => m.state.status,
  });
  const signingIn = logins.includes('pending');
  const signInFailed = !signingIn && logins.at(-1) === 'error';

  const accounts = query.data?.accounts ?? [];
  const active =
    accounts.find((a) => a.uuid === query.data?.defaultUuid) ?? accounts[0];

  return {
    accounts,
    active,
    signedIn: active ? !active.needsReauth : false,
    signingIn,
    signInFailed,
    isPending: query.isPending,
    ready: !query.isPending,
    login,
    beginLogin,
    completeLogin,
    switch: switchAccount,
    remove,
  };
}

export async function ensureSignedIn(
  queryClient: QueryClient,
): Promise<boolean> {
  const list = await queryClient.ensureQueryData(accountQueries.list());
  return list.accounts.some((a) => !a.needsReauth);
}
