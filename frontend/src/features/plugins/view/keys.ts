const SEPARATOR = '\u0000';

/** Pairs each item with a key from what it says; items that say the same thing are told apart by their order among them. */
export function keyed<T>(
  items: T[],
  identify: (item: T) => string = shallowIdentity,
): Array<{ key: string; item: T }> {
  const seen = new Map<string, number>();
  return items.map((item) => {
    const identity = identify(item);
    const nth = seen.get(identity) ?? 0;
    seen.set(identity, nth + 1);
    return { key: `${identity}${SEPARATOR}${nth}`, item };
  });
}

/** An item's own scalar fields: nested children are left out so a node keeps its identity (and its state) while its subtree changes. */
function shallowIdentity(item: unknown): string {
  if (typeof item !== 'object' || item === null) return String(item);
  return JSON.stringify(
    Object.entries(item).filter(([, value]) => typeof value !== 'object'),
  );
}
