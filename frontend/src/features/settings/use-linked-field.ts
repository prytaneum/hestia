import { getRouteApi } from '@tanstack/react-router';
import { useEffect } from 'react';

const route = getRouteApi('/_app/settings/');

/**
 * Brings the field a link named (`?field=<element id>`) into view and focuses
 * it. Called by whatever renders linkable fields, so it runs once they exist.
 */
export function useLinkedField() {
  const { field } = route.useSearch();
  useEffect(() => {
    if (!field) return;
    const element = document.getElementById(field);
    element?.scrollIntoView({ block: 'center' });
    element?.focus({ preventScroll: true });
  }, [field]);
}
