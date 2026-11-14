/**
 * How a provider id is shown. The daemon speaks full ids (`<plugin>.<id>`); a
 * person reads the provider-id alone while no other registered provider shares
 * it, which is also when the daemon accepts that bare name.
 */
export function providerNamer(ids: Iterable<string>): (id: string) => string {
  const known = new Set(ids);
  const bare = (id: string) => id.slice(id.indexOf('.') + 1);
  const counts = new Map<string, number>();
  for (const id of known) counts.set(bare(id), (counts.get(bare(id)) ?? 0) + 1);
  return (id) => (known.has(id) && counts.get(bare(id)) === 1 ? bare(id) : id);
}
