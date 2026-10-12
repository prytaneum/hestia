import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  ANY_FILE,
  activeDropzone,
  type Dropzone,
  dispatchDrop,
  registerDropzone,
  subscribeDropzone,
} from '@/lib/file-drop';

const cleanups: (() => void)[] = [];
const position = { x: 10, y: 10 };

function element(parent: Element = document.body): HTMLElement {
  const created = document.createElement('div');
  parent.appendChild(created);
  cleanups.push(() => created.remove());
  return created;
}

function zone(on: Element, accept: string[], options: Partial<Dropzone> = {}) {
  const onDrop = vi.fn();
  const registered: Dropzone = { accept, onDrop, multiple: true, ...options };
  cleanups.push(registerDropzone(on, registered));
  return { registered, onDrop };
}

/** Put the pointer over an element, as far as the hit test can tell. */
function pointAt(target: Element | null): void {
  document.elementFromPoint = vi.fn(() => target);
}

afterEach(() => {
  dispatchDrop({ type: 'leave' });
  for (const cleanup of cleanups.splice(0)) cleanup();
});

describe('file-drop pipeline', () => {
  it('hands every file to a zone that accepts any', () => {
    const root = element();
    const tree = zone(root, [ANY_FILE]);
    pointAt(element(root));

    dispatchDrop({
      type: 'drop',
      paths: ['/a/plugin.jar', '/a/README'],
      position,
    });

    expect(tree.onDrop).toHaveBeenCalledWith(['/a/plugin.jar', '/a/README']);
  });

  it('hands a drop to the zone under the pointer, with only the accepted paths', () => {
    const root = element();
    const archives = zone(root, ['mrpack', 'zip']);
    pointAt(element(root));

    dispatchDrop({
      type: 'drop',
      paths: ['/a/pack.MRPACK', '/a/notes.txt'],
      position,
    });

    expect(archives.onDrop).toHaveBeenCalledWith(['/a/pack.MRPACK']);
  });

  it('takes only the first accepted file when the zone is not multiple', () => {
    const root = element();
    const archives = zone(root, ['zip'], { multiple: false });
    pointAt(root);

    dispatchDrop({ type: 'drop', paths: ['/a/one.zip', '/a/two.zip'], position });

    expect(archives.onDrop).toHaveBeenCalledWith(['/a/one.zip']);
  });

  it('prefers the innermost zone', () => {
    const outer = element();
    const inner = element(outer);
    const page = zone(outer, ['zip']);
    const button = zone(inner, ['zip']);
    pointAt(inner);

    dispatchDrop({ type: 'drop', paths: ['/a/pack.zip'], position });

    expect(button.onDrop).toHaveBeenCalledOnce();
    expect(page.onDrop).not.toHaveBeenCalled();
  });

  it('bubbles to an outer zone when the inner one does not take the file', () => {
    const outer = element();
    const inner = element(outer);
    const page = zone(outer, ['mrpack']);
    const button = zone(inner, ['jar']);
    pointAt(inner);

    dispatchDrop({ type: 'drop', paths: ['/a/pack.mrpack'], position });

    expect(page.onDrop).toHaveBeenCalledWith(['/a/pack.mrpack']);
    expect(button.onDrop).not.toHaveBeenCalled();
  });

  it('never reaches a zone the pointer is not over', () => {
    const page = zone(element(), ['zip']);
    // A dialog is a sibling subtree portaled over the page.
    pointAt(element());

    dispatchDrop({ type: 'drop', paths: ['/a/pack.zip'], position });

    expect(page.onDrop).not.toHaveBeenCalled();
  });

  it('follows the pointer between zones while dragging', () => {
    const left = element();
    const right = element();
    const first = zone(left, ['zip'], { label: 'Drop to import' });
    const second = zone(right, ['zip']);
    const changed = vi.fn();
    cleanups.push(subscribeDropzone(changed));

    pointAt(left);
    dispatchDrop({ type: 'enter', paths: ['/a/pack.zip'], position });
    expect(activeDropzone()).toBe(first.registered);

    pointAt(right);
    dispatchDrop({ type: 'over', position });
    expect(activeDropzone()).toBe(second.registered);

    dispatchDrop({ type: 'leave' });
    expect(activeDropzone()).toBeNull();
    expect(changed).toHaveBeenCalledTimes(3);
  });

  it('stops routing to a zone once it is unregistered', () => {
    const root = element();
    const onDrop = vi.fn();
    const unregister = registerDropzone(root, {
      accept: ['zip'],
      onDrop,
      multiple: true,
    });
    pointAt(root);

    dispatchDrop({ type: 'enter', paths: ['/a/pack.zip'], position });
    unregister();
    expect(activeDropzone()).toBeNull();

    dispatchDrop({ type: 'drop', paths: ['/a/pack.zip'], position });
    expect(onDrop).not.toHaveBeenCalled();
  });
});
