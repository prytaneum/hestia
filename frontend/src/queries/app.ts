/** `app.*` — query factories, consumed through `useQuery`. */
import { queryOptions } from '@tanstack/react-query';
import * as api from '../api/app';
import { keys } from './keys';

export const appQueries = {
  info: () =>
    queryOptions({
      queryKey: keys.app.info(),
      queryFn: () => api.info(),
      staleTime: Number.POSITIVE_INFINITY,
    }),
};
