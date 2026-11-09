import type { Edition, EditionRules, Flavor, KeyRule } from '@/api';
import { m } from '@/paraglide/messages.js';

export const editionLabel: Record<Edition, () => string> = {
  java: m['domain.edition.java'],
  bedrock: m['domain.edition.bedrock'],
};

/** Flavors grouped by edition, in the order the daemon first lists each edition. */
export function groupByEdition(
  flavors: Flavor[],
): Array<{ edition: Edition; flavors: Flavor[] }> {
  const groups = new Map<Edition, Flavor[]>();
  for (const flavor of flavors) {
    groups.set(flavor.edition, [...(groups.get(flavor.edition) ?? []), flavor]);
  }
  return [...groups].map(([edition, list]) => ({ edition, flavors: list }));
}

/** The property rules the daemon sent for one edition. */
export const rulesFor = (
  all: EditionRules[],
  edition: Edition | undefined,
): KeyRule[] => all.find((r) => r.edition === edition)?.rules ?? [];

export const hasRule = (rules: KeyRule[], key: string): boolean =>
  rules.some((rule) => rule.key === key);

/** The property that carries a server's listed name; the rules hold no string keys to read it from. */
export const serverNameKey = (edition: Edition | undefined) =>
  edition === 'bedrock' ? 'server-name' : 'motd';

/** The values a `choice` rule allows, or null when the key has no such rule. */
export function choiceValues(rules: KeyRule[], key: string): string[] | null {
  const kind = rules.find((rule) => rule.key === key)?.kind;
  return kind?.type === 'choice' ? kind.values : null;
}
