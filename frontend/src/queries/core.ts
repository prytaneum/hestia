/**
 * The plain-mutation factory: typed `UseMutationOptions` over an API call,
 * with invalidation declared as data. Every non-job mutation in the domain
 * files is one `mutation({ ... })`; job-backed operations use `jobMutation`
 * from `./jobs` instead.
 */
import type { QueryKey, UseMutationOptions } from '@tanstack/react-query';
import type { HestiaError } from '../api';
import { invalidate, queryClient } from './client';

/** Staleness for the upstream catalogue reads (flavors/versions/loaders/profile). */
export const CATALOG_STALE_MS = 5 * 60_000;

export interface MutationSpec<TData, TVariables> {
  mutationKey: QueryKey;
  mutationFn: (variables: TVariables) => Promise<TData>;
  /** Applies the expected outcome to the cache now; the undo runs on error. */
  optimistic?: (variables: TVariables) => (() => void) | undefined;
  /** Seed the cache from the server's answer before the settle invalidation. */
  onSuccess?: (data: TData, variables: TVariables) => void;
  /** Key prefixes swept when the mutation settles, success or error. */
  invalidates?: (variables: TVariables) => QueryKey[];
}

/** Writes `update(cached)` under `key` now; returns the undo, or nothing when the key is uncached. */
export function optimisticUpdate<T>(
  key: QueryKey,
  update: (previous: T) => T,
): (() => void) | undefined {
  void queryClient.cancelQueries({ queryKey: key });
  const previous = queryClient.getQueryData<T>(key);
  if (!previous) return undefined;
  queryClient.setQueryData<T>(key, update(previous));
  return () => queryClient.setQueryData(key, previous);
}

interface OptimisticContext {
  undo?: () => void;
}

export function mutation<TData = void, TVariables = void>(
  spec: MutationSpec<TData, TVariables>,
): UseMutationOptions<TData, HestiaError, TVariables, OptimisticContext> {
  return {
    mutationKey: spec.mutationKey,
    mutationFn: spec.mutationFn,
    onMutate: spec.optimistic
      ? (variables) => ({ undo: spec.optimistic?.(variables) })
      : undefined,
    onSuccess: spec.onSuccess,
    onError: (_error, _variables, context) => {
      context?.undo?.();
    },
    onSettled: (_data, _error, variables) => {
      for (const key of spec.invalidates?.(variables) ?? []) invalidate(key);
    },
  };
}
