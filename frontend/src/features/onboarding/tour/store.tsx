import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useState,
} from 'react';

import { useOnboarding } from '../state';
import { type TourId, type TourStep, tours } from './registry';
import { Spotlight } from './spotlight';

interface TourRun {
  id: TourId;
  index: number;
}

export interface TourControls {
  run: TourRun | null;
  steps: readonly TourStep[];
  step: TourStep | null;
  start: (id: TourId) => void;
  go: (index: number) => void;
  next: () => void;
  back: () => void;
  stop: () => void;
  seen: (id: TourId) => boolean;
  idle: boolean;
}

const TourContext = createContext<TourControls | null>(null);

export function useTour(): TourControls {
  const controls = useContext(TourContext);
  if (!controls) throw new Error('useTour used outside TourProvider');
  return controls;
}

export function TourProvider({ children }: { children: ReactNode }) {
  const onboarding = useOnboarding();
  const [run, setRun] = useState<TourRun | null>(null);

  const { seen, welcomed, ready, update } = onboarding;

  const start = useCallback(
    (id: TourId) => {
      setRun({ id, index: 0 });
      if (!seen.includes(id)) update({ seen: [...seen, id] });
    },
    [seen, update],
  );

  const controls = useMemo<TourControls>(() => {
    const steps = run ? tours[run.id] : [];
    const stop = () => setRun(null);
    const go = (index: number) => {
      if (index < 0 || index >= steps.length) stop();
      else setRun((current) => (current ? { ...current, index } : current));
    };

    return {
      run,
      steps,
      step: run ? (steps[run.index] ?? null) : null,
      start,
      go,
      next: () => go((run?.index ?? 0) + 1),
      back: () => go((run?.index ?? 0) - 1),
      stop,
      seen: (id) => seen.includes(id),
      idle: ready && welcomed && run === null,
    };
  }, [run, start, seen, ready, welcomed]);

  return (
    <TourContext.Provider value={controls}>
      {children}
      <Spotlight />
    </TourContext.Provider>
  );
}
