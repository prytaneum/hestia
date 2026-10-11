import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  activeDropTarget,
  type DropTarget,
  dispatchDrop,
  registerDropTarget,
  subscribeDropTarget,
} from '@/lib/file-drop';

const cleanups: (() => void)[] = [];

function target(extensions: string[], label?: string) {
  const onDrop = vi.fn();
  const registered: DropTarget = { extensions, onDrop, label };
  cleanups.push(registerDropTarget(registered));
  return { registered, onDrop };
}

afterEach(() => {
  dispatchDrop({ type: 'leave' });
  for (const cleanup of cleanups.splice(0)) cleanup();
});

describe('file-drop pipeline', () => {
  it('hands a drop to the target that accepts it, with only the matching paths', () => {
    const archives = target(['mrpack', 'zip']);

    dispatchDrop({ type: 'drop', paths: ['/a/pack.MRPACK', '/a/notes.txt'] });

    expect(archives.onDrop).toHaveBeenCalledWith(['/a/pack.MRPACK']);
  });

  it('routes to the most recently registered target', () => {
    const page = target(['zip']);
    const dialog = target(['zip']);

    dispatchDrop({ type: 'drop', paths: ['/a/pack.zip'] });

    expect(dialog.onDrop).toHaveBeenCalledOnce();
    expect(page.onDrop).not.toHaveBeenCalled();
  });

  it('falls through to a lower target when the top one does not take the file', () => {
    const page = target(['mrpack']);
    const dialog = target(['jar']);

    dispatchDrop({ type: 'drop', paths: ['/a/pack.mrpack'] });

    expect(page.onDrop).toHaveBeenCalledWith(['/a/pack.mrpack']);
    expect(dialog.onDrop).not.toHaveBeenCalled();
  });

  it('drops nothing when no target accepts the files', () => {
    const archives = target(['mrpack']);

    dispatchDrop({ type: 'drop', paths: ['/a/photo.png'] });

    expect(archives.onDrop).not.toHaveBeenCalled();
  });

  it('names the target a drag would land on until the drag ends', () => {
    const page = target(['zip'], 'Drop to import');
    const changed = vi.fn();
    cleanups.push(subscribeDropTarget(changed));

    dispatchDrop({ type: 'enter', paths: ['/a/pack.zip'] });
    expect(activeDropTarget()).toBe(page.registered);

    dispatchDrop({ type: 'over' });
    expect(activeDropTarget()).toBe(page.registered);

    dispatchDrop({ type: 'leave' });
    expect(activeDropTarget()).toBeNull();
    expect(changed).toHaveBeenCalledTimes(2);
  });

  it('stops routing to a target once it is unregistered', () => {
    const page = target(['zip']);
    const onDrop = vi.fn();
    const unregister = registerDropTarget({ extensions: ['zip'], onDrop });

    dispatchDrop({ type: 'enter', paths: ['/a/pack.zip'] });
    unregister();
    expect(activeDropTarget()).toBeNull();

    dispatchDrop({ type: 'drop', paths: ['/a/pack.zip'] });
    expect(onDrop).not.toHaveBeenCalled();
    expect(page.onDrop).toHaveBeenCalledOnce();
  });
});
