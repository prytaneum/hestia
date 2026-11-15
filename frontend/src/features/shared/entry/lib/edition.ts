import type { Edition, EditionCatalogue, KeyRule } from '@/api';
import { m } from '@/paraglide/messages.js';

export const editionLabel: Record<Edition, () => string> = {
  java: m['domain.edition.java'],
  bedrock: m['domain.edition.bedrock'],
};

/** The property rules the daemon sent for one edition. */
export const rulesFor = (
  all: EditionCatalogue[],
  edition: Edition | undefined,
): KeyRule[] => all.find((r) => r.edition === edition)?.rules ?? [];

export const hasRule = (rules: KeyRule[], key: string): boolean =>
  rules.some((rule) => rule.key === key);

/** The values a `choice` rule allows, or null when the key has no such rule. */
export function choiceValues(rules: KeyRule[], key: string): string[] | null {
  const kind = rules.find((rule) => rule.key === key)?.kind;
  return kind?.type === 'choice' ? kind.values : null;
}
