import { z } from 'zod';
import { m } from '@/paraglide/messages.js';
import type { MemoryRange } from '@/queries/host';

type Kind = 'server' | 'instance';

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

export function detailsStepSchema(kind: Kind, memory: MemoryRange) {
  return z.object({
    name: z.string(),
    memory: z.number().min(memory.min).max(memory.max),
    motd: z.string(),
    seed: z.string(),
    gamemode: z.string(),
    difficulty: z.string(),
    maxPlayers: z
      .string()
      .regex(/^\d+$/, m['app.validation.whole_number']())
      .refine((v) => Number(v) >= 1, m['app.validation.min_players']()),
    port: z
      .string()
      .regex(/^\d*$/, m['app.validation.port_number']())
      .refine(
        (v) => v === '' || Number(v) <= 65535,
        m['app.validation.port_range'](),
      ),
    hardcore: z.boolean(),
    onlineMode: z.boolean(),
    eula:
      kind === 'server'
        ? z.literal(true, { error: m['app.validation.eula']() })
        : z.boolean(),
  });
}

export function createWizardSchema(kind: Kind, memory: MemoryRange) {
  return z.object({
    flavor: flavorStepSchema(),
    version: versionStepSchema(),
    details: detailsStepSchema(kind, memory),
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
