import { CaretLeftIcon, CaretRightIcon } from '@phosphor-icons/react';
import {
  type ContentKind,
  type ContentProject,
  errorMessageFromInfo,
} from '@/api';
import { AppDialog, DialogBody, DialogFooter } from '@/components/dialog';
import { contentIcon, entryIcon } from '@/components/icons';
import { StepDots } from '@/components/step-dots';
import { Button } from '@/components/ui/button';
import {
  Progress,
  ProgressLabel,
  ProgressValue,
} from '@/components/ui/progress';
import { projectKey, projectRef } from '@/features/content/components';
import { m } from '@/paraglide/messages.js';
import { instanceMutations } from '@/queries/instance';
import { useJobDisplay, useJobMutation } from '@/queries/jobs';
import { serverMutations } from '@/queries/server';
import { TargetCtx, useInstallWizard } from './hooks';
import { type Target, targetTakesKind, useTargets } from './lib';
import { ContentStep } from './steps/content';
import { ReviewStep } from './steps/review';
import { TargetStep } from './steps/target';
import { WorldsStep } from './steps/worlds';

/**
 * The content install modal over the daemon's `content.add`. It opens either way
 * round: from Browse a `project` is fixed and the user picks a target; from an
 * entry's page the `entry` is
 * fixed and the user picks a project. The newest compatible version resolves
 * automatically (changeable in the review), a datapack on an instance chooses
 * its worlds, and the install runs as a job with live progress.
 */
export function ContentInstallDialog({
  project,
  pinnedVersion,
  entry,
  open,
  onOpenChange,
}: {
  project?: ContentProject;
  pinnedVersion?: string;
  entry?: Target;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const mode: 'browse' | 'entry' = project ? 'browse' : 'entry';
  const targets = useTargets();

  const [state, dispatch] = useInstallWizard({
    open,
    entryId: entry?.id ?? '',
    project,
    versionId: pinnedVersion,
  });
  const {
    step,
    targetId,
    picked,
    files,
    kindFilter,
    source,
    versionIds,
    worlds,
    installing,
    error,
  } = state;

  const addServer = useJobMutation(serverMutations.content.add(targetId));
  const addInstance = useJobMutation(instanceMutations.content.add(targetId));
  useJobDisplay(addServer.job, open);
  useJobDisplay(addInstance.job, open);

  const target = targets.find((t) => t.id === targetId) ?? entry ?? null;
  const selectedCount = picked.length + files.length;
  const fileKinds = files
    .map((f) => f.kind)
    .filter((k): k is ContentKind => k !== undefined);
  const selectedKinds = [
    ...new Set([...picked.map((p) => p.kind), ...fileKinds]),
  ];
  // A staged file blocks install until it is installable and has a kind.
  const filesReady = files.every((f) => f.valid && f.kind !== undefined);
  const needsWorlds =
    selectedKinds.includes('data_pack') && target?.type === 'instance';

  // The toggle clears any pin, so a new one is set after it.
  const toggleProject = (p: ContentProject, versionId?: string) => {
    dispatch({ type: 'toggleProject', project: p });
    if (versionId) {
      dispatch({ type: 'version', ref: projectKey(p), id: versionId });
    }
  };

  const pickStep = mode === 'browse' ? 'target' : 'content';
  const steps: string[] = needsWorlds
    ? [pickStep, 'worlds', 'review']
    : [pickStep, 'review'];
  const stepId = steps[Math.min(step, steps.length - 1)];

  const Icon =
    mode === 'browse' && project
      ? contentIcon(project.kind)
      : entryIcon(entry?.type ?? 'instance');
  const title =
    mode === 'browse'
      ? m['content.install_title']({ name: project?.title ?? '' })
      : m['content.add_to_title']({ name: entry?.name ?? '' });

  const hint = installing
    ? m['content.installing']()
    : stepId === 'target'
      ? m['content.hint.target']()
      : stepId === 'content'
        ? m['content.hint.content']()
        : stepId === 'worlds'
          ? m['content.hint.worlds']()
          : m['content.hint.review']();

  const canAdvance =
    stepId === 'target'
      ? !!targetId
      : stepId === 'content'
        ? selectedCount > 0
        : stepId === 'worlds'
          ? worlds.length > 0
          : selectedCount > 0 && filesReady;

  const liveProgress =
    (target?.type === 'server' ? addServer : addInstance).progress ?? null;
  const progressPct =
    liveProgress && liveProgress.total > 0
      ? Math.round((liveProgress.current / liveProgress.total) * 100)
      : 0;
  const progressPhase = liveProgress?.detail || liveProgress?.phase || '';

  async function install() {
    if (!target) return;
    dispatch({ type: 'installStart' });
    try {
      const add = target.type === 'server' ? addServer : addInstance;
      // The wire spec is per-kind, so a mixed selection installs as one batch
      // per kind; failures aggregate across batches.
      const failures: string[] = [];
      for (const k of selectedKinds) {
        // Each project names its own source, so one batch may mix platforms.
        const items = [
          ...picked
            .filter((p) => p.kind === k)
            .map((p) => ({
              project: projectRef(p),
              source: p.source,
              version: versionIds[projectKey(p)] ?? '',
            })),
          ...files
            .filter((f) => f.kind === k)
            .map((f) => ({ path: f.path, source: f.track })),
        ];
        const done = await add.mutateAsync({
          kind: k,
          items,
          worlds: k === 'data_pack' && needsWorlds ? worlds : [],
        });
        failures.push(
          ...done.failures.map((f) => errorMessageFromInfo(f.error)),
        );
      }
      if (failures.length > 0) {
        dispatch({ type: 'installError', message: failures.join('; ') });
        return;
      }
      onOpenChange(false);
    } catch (e) {
      dispatch({
        type: 'installError',
        message: e instanceof Error ? e.message : String(e),
      });
    }
  }

  return (
    <TargetCtx.Provider value={target}>
      <AppDialog
        open={open}
        onOpenChange={(o) => {
          if (!installing) onOpenChange(o);
        }}
        size="xl"
        height="tall"
        icon={Icon}
        title={title}
        description={hint}
      >
        {installing ? (
          <DialogBody className="flex flex-col justify-center">
            <Progress value={progressPct}>
              <ProgressLabel>
                {progressPhase || m['content.installing']()}
              </ProgressLabel>
              <ProgressValue />
            </Progress>
          </DialogBody>
        ) : (
          <>
            <DialogBody className="flex flex-col overflow-hidden">
              {stepId === 'target' ? (
                <TargetStep
                  kind={project?.kind ?? 'mod'}
                  targets={targets.filter(
                    (t) => project && targetTakesKind(t, project.kind),
                  )}
                  selectedId={targetId}
                  onSelect={(t) => dispatch({ type: 'target', id: t.id })}
                />
              ) : stepId === 'content' ? (
                target && (
                  <ContentStep
                    target={target}
                    kind={kindFilter}
                    onKindChange={(kind) =>
                      dispatch({ type: 'kindFilter', kind })
                    }
                    source={source}
                    onSourceChange={(next) =>
                      dispatch({ type: 'source', source: next })
                    }
                    picked={picked}
                    onToggle={toggleProject}
                    onAddFiles={(files) =>
                      dispatch({ type: 'addFiles', files })
                    }
                  />
                )
              ) : stepId === 'worlds' ? (
                <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
                  <WorldsStep
                    instanceId={target?.id ?? ''}
                    selected={worlds}
                    onToggle={(world, on) =>
                      dispatch({ type: 'toggleWorld', world, on })
                    }
                  />
                </div>
              ) : (
                <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
                  <ReviewStep
                    picked={picked}
                    files={files}
                    versionIds={versionIds}
                    onVersion={(ref, id) =>
                      dispatch({ type: 'version', ref, id })
                    }
                    onRemoveProject={toggleProject}
                    onRemoveFile={(path) =>
                      dispatch({ type: 'removeFile', path })
                    }
                    onSetFileKind={(path, kind) =>
                      dispatch({ type: 'setFileKind', path, kind })
                    }
                    onSetFileTrack={(path, track) =>
                      dispatch({ type: 'setFileTrack', path, track })
                    }
                    worlds={needsWorlds ? worlds : undefined}
                  />
                </div>
              )}
              {error && (
                <p className="mt-3 shrink-0 px-1 text-xs text-destructive">
                  {error}
                </p>
              )}
            </DialogBody>

            <DialogFooter className="items-center">
              <div className="mr-auto flex items-center gap-3">
                <StepDots
                  steps={steps}
                  active={Math.min(step, steps.length - 1)}
                />
                {mode === 'entry' && selectedCount > 0 && (
                  <span className="text-[11px] text-muted-foreground">
                    {m['content.selected_count']({ count: selectedCount })}
                  </span>
                )}
              </div>
              {step === 0 ? (
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => onOpenChange(false)}
                >
                  {m['app.action.cancel']()}
                </Button>
              ) : (
                <Button
                  type="button"
                  variant="outline"
                  data-icon="inline-start"
                  onClick={() =>
                    dispatch({ type: 'step', step: Math.max(0, step - 1) })
                  }
                >
                  <CaretLeftIcon />
                  {m['app.action.back']()}
                </Button>
              )}
              {stepId === 'review' ? (
                <Button
                  type="button"
                  disabled={!canAdvance}
                  className="bg-ember text-ember-foreground hover:bg-ember/90"
                  onClick={install}
                >
                  {m['app.action.install']()}
                </Button>
              ) : (
                <Button
                  type="button"
                  data-icon="inline-end"
                  disabled={!canAdvance}
                  className="bg-ember text-ember-foreground hover:bg-ember/90"
                  onClick={() => dispatch({ type: 'step', step: step + 1 })}
                >
                  {m['app.action.next']()}
                  <CaretRightIcon />
                </Button>
              )}
            </DialogFooter>
          </>
        )}
      </AppDialog>
    </TargetCtx.Provider>
  );
}
