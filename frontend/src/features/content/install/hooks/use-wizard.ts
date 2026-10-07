import { useEffect, useReducer } from 'react';

import type { ContentKind, ContentProject } from '@/api';
import { projectKey } from '@/features/content/components';
import type { PickedFile } from '../lib';

export interface WizardInit {
  open: boolean;
  entryId: string;
  project?: ContentProject;
  versionId?: string;
}

export interface WizardState {
  step: number;
  targetId: string;
  picked: ContentProject[];
  files: PickedFile[];
  kindFilter: ContentKind | null;
  /** Which source the picker browses; empty is the daemon's default. */
  source: string;
  versionIds: Record<string, string>;
  worlds: string[];
  installing: boolean;
  error: string;
}

export type WizardAction =
  | { type: 'reset'; init: WizardInit }
  | { type: 'step'; step: number }
  | { type: 'target'; id: string }
  | { type: 'toggleProject'; project: ContentProject }
  | { type: 'addFiles'; files: PickedFile[] }
  | { type: 'setFileKind'; path: string; kind: ContentKind }
  | { type: 'setFileTrack'; path: string; track: string }
  | { type: 'removeFile'; path: string }
  | { type: 'kindFilter'; kind: ContentKind | null }
  | { type: 'source'; source: string }
  | { type: 'version'; ref: string; id: string }
  | { type: 'toggleWorld'; world: string; on: boolean }
  | { type: 'installStart' }
  | { type: 'installError'; message: string };

function initial(init: WizardInit): WizardState {
  return {
    step: 0,
    targetId: init.entryId,
    picked: init.project ? [init.project] : [],
    files: [],
    kindFilter: null,
    source: init.project?.source ?? '',
    versionIds:
      init.project && init.versionId
        ? { [projectKey(init.project)]: init.versionId }
        : {},
    worlds: [],
    installing: false,
    error: '',
  };
}

function reducer(state: WizardState, action: WizardAction): WizardState {
  switch (action.type) {
    case 'reset':
      return initial(action.init);
    case 'step':
      return { ...state, step: action.step };
    case 'target':
      return { ...state, targetId: action.id, worlds: [] };
    case 'toggleProject': {
      const key = projectKey(action.project);
      const has = state.picked.some((p) => projectKey(p) === key);
      const { [key]: _dropped, ...versionIds } = state.versionIds;
      return {
        ...state,
        picked: has
          ? state.picked.filter((p) => projectKey(p) !== key)
          : [...state.picked, action.project],
        versionIds,
      };
    }
    case 'addFiles':
      return {
        ...state,
        files: [
          ...state.files,
          ...action.files.filter(
            (f) => !state.files.some((existing) => existing.path === f.path),
          ),
        ],
      };
    case 'setFileKind':
      return {
        ...state,
        files: state.files.map((f) =>
          f.path === action.path ? { ...f, kind: action.kind } : f,
        ),
      };
    case 'setFileTrack':
      return {
        ...state,
        files: state.files.map((f) =>
          f.path === action.path ? { ...f, track: action.track } : f,
        ),
      };
    case 'removeFile':
      return {
        ...state,
        files: state.files.filter((f) => f.path !== action.path),
      };
    case 'kindFilter':
      return { ...state, kindFilter: action.kind };
    case 'source':
      return { ...state, source: action.source };
    case 'version': {
      const { [action.ref]: _dropped, ...rest } = state.versionIds;
      return {
        ...state,
        versionIds: action.id ? { ...rest, [action.ref]: action.id } : rest,
      };
    }
    case 'toggleWorld':
      return {
        ...state,
        worlds: action.on
          ? [...state.worlds, action.world]
          : state.worlds.filter((w) => w !== action.world),
      };
    case 'installStart':
      return { ...state, installing: true, error: '' };
    case 'installError':
      return { ...state, installing: false, error: action.message };
  }
}

/**
 * Owns the install modal's step/selection/progress state. Reset runs on the
 * open transition only, so a fixed `project`/`entry` seeds the first render and
 * later changes don't clobber an in-progress selection.
 */
export function useInstallWizard(init: WizardInit) {
  const [state, dispatch] = useReducer(reducer, init, initial);
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset only on open.
  useEffect(() => {
    if (init.open) dispatch({ type: 'reset', init });
  }, [init.open]);
  return [state, dispatch] as const;
}
