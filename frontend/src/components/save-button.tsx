import { CheckIcon, CircleNotchIcon } from '@phosphor-icons/react';
import type { ComponentProps } from 'react';

import { Button } from '@/components/ui/button';
import type { SaveState } from '@/hooks/use-save-state';
import { m } from '@/paraglide/messages.js';

function stateLabel(state: SaveState): string {
  if (state === 'saving') return m['app.status.saving']();
  if (state === 'saved') return m['app.status.saved']();
  return m['app.action.save']();
}

/**
 * The announcement for a save. It must be rendered before the state changes,
 * so it lives beside the button rather than inside something that appears with it.
 */
export function SaveStatus({ state }: { state: SaveState }) {
  return (
    <span role="status" className="sr-only">
      {state === 'idle' ? '' : stateLabel(state)}
    </span>
  );
}

/** Save, Saving…, Saved at one fixed width so the row never shifts. */
export function SaveButton({
  state,
  disabled,
  ...props
}: { state: SaveState } & ComponentProps<typeof Button>) {
  return (
    <Button
      size="sm"
      aria-label={m['app.action.save']()}
      disabled={disabled || state === 'saving'}
      {...props}
      className="w-24 shrink-0"
    >
      {state === 'saving' && (
        <CircleNotchIcon
          aria-hidden
          className="size-4 animate-spin motion-reduce:animate-none"
        />
      )}
      {state === 'saved' && <CheckIcon aria-hidden className="size-4" />}
      <span aria-hidden>{stateLabel(state)}</span>
    </Button>
  );
}
