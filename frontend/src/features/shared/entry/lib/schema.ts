import { z } from 'zod';
import type { KeyRule } from '@/api/types';
import { m } from '@/paraglide/messages.js';
import type { MemoryRange } from '@/queries/host';

type Kind = 'server' | 'instance';

const WHOLE_NUMBER = /^-?\d+$/;

function integerField(rules: KeyRule[], key: string, optional = false) {
  const kind = rules.find((rule) => rule.key === key)?.kind;
  const range = kind?.type === 'integer' ? kind : null;
  return z
    .string()
    .refine(
      (value) => (optional && value === '') || WHOLE_NUMBER.test(value),
      m['app.validation.whole_number'](),
    )
    .refine(
      (value) =>
        !range ||
        !WHOLE_NUMBER.test(value) ||
        (Number(value) >= range.min && Number(value) <= range.max),
      range ? m['app.validation.out_of_range'](range) : '',
    );
}

/**
 * Zod schemas for the create wizard, one per step. Each `FormGroup`'s
 * `onDynamic` validator uses its step schema, so "Next" only gates on the
 * current step; the whole-form `onDynamic` composes them for the final submit.
 */

export function flavorStepSchema() {
  return z.object({
    flavor: z.string().min(1, m['app.validation.pick_flavor']()),
  });
}

export function versionStepSchema() {
  return z.object({
    version: z.string().min(1, m['app.validation.choose_version']()),
    loaderVersion: z.string(),
  });
}

export function detailsStepSchema(
  kind: Kind,
  memory: MemoryRange,
  rules: KeyRule[],
) {
  return z.object({
    name: z.string(),
    memory: z.number().min(memory.min).max(memory.max),
    motd: z.string(),
    seed: z.string(),
    gamemode: z.string(),
    difficulty: z.string(),
    maxPlayers: integerField(rules, 'max-players'),
    port: integerField(rules, 'server-port', true),
    hardcore: z.boolean(),
    onlineMode: z.boolean(),
    eula:
      kind === 'server'
        ? z.literal(true, { error: m['app.validation.eula']() })
        : z.boolean(),
  });
}

export function createWizardSchema(
  kind: Kind,
  memory: MemoryRange,
  rules: KeyRule[],
) {
  return z.object({
    flavor: flavorStepSchema(),
    version: versionStepSchema(),
    details: detailsStepSchema(kind, memory, rules),
  });
}

/** The wizard's collected form value — the shape both `create` params read. */
export type WizardValues = ReturnType<typeof createWizardDefaults>;

export function createWizardDefaults(
  loaderVersion: string,
  defaultMemoryGb = 4,
) {
  return {
    flavor: { flavor: 'vanilla' },
    version: { version: '', loaderVersion },
    details: {
      name: '',
      memory: defaultMemoryGb,
      motd: 'A Minecraft Server',
      seed: '',
      gamemode: 'survival',
      difficulty: 'normal',
      maxPlayers: '20',
      port: '',
      hardcore: false,
      onlineMode: true,
      eula: false,
    },
  };
}
