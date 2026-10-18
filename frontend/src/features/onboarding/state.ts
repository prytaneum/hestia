import { usePrefs } from '@/queries/prefs';

import type { TourId } from './tour/registry';

const ONBOARDING_KEY = 'onboarding';

export interface OnboardingState {
  welcomed: boolean;
  seen: TourId[];
  checklistDismissed: boolean;
}

const FRESH: OnboardingState = {
  welcomed: false,
  seen: [],
  checklistDismissed: false,
};

function normalize(value: unknown): OnboardingState {
  if (typeof value !== 'object' || value === null) return FRESH;
  const raw = value as Partial<Record<keyof OnboardingState, unknown>>;
  return {
    welcomed: raw.welcomed === true,
    seen: Array.isArray(raw.seen)
      ? raw.seen.filter((id): id is TourId => typeof id === 'string')
      : [],
    checklistDismissed: raw.checklistDismissed === true,
  };
}

export interface Onboarding extends OnboardingState {
  ready: boolean;
  update: (patch: Partial<OnboardingState>) => void;
}

export function useOnboarding(): Onboarding {
  const prefs = usePrefs();
  const state = normalize(prefs.get<unknown>(ONBOARDING_KEY, null));

  return {
    ...state,
    ready: prefs.ready,
    update: (patch) => prefs.set(ONBOARDING_KEY, { ...state, ...patch }),
  };
}
