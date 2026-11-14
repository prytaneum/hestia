import { screen, waitFor } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { act } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Action, UiNode } from '@/api/types';
import { ViewCtx } from '@/features/plugins/view/context';
import { UiView } from '@/features/plugins/view/render';
import { PluginView } from '@/features/plugins/view';
import { publish } from '@/mock/bus';
import { channels } from '@/mock/channels';
import { m } from '@/paraglide/messages.js';
import {
  installFakeDaemon,
  renderWithProviders,
  resetQueryCache,
} from '../../../support';

afterEach(() => {
  vi.useRealTimers();
  resetQueryCache();
});

function show(node: UiNode, run = vi.fn<(action: Action) => Promise<void>>()) {
  renderWithProviders(
    <ViewCtx.Provider value={{ plugin: 'hello', pending: false, run }}>
      <UiView node={node} />
    </ViewCtx.Provider>,
  );
  return run;
}

const button = (confirm?: string): UiNode =>
  ({
    type: 'button',
    label: 'Reset',
    action: { id: 'reset', payload: null },
    variant: 'danger',
    confirm,
  }) as UiNode;

describe('plugin UI primitives', () => {
  it('draws a badge with its text', () => {
    show({ type: 'badge', text: 'Online', tone: 'success' });
    expect(screen.getByText('Online')).toBeTruthy();
  });

  it('shows the first tab and switches to another', async () => {
    show({
      type: 'tabs',
      tabs: [
        { id: 'a', label: 'Alpha', children: [{ type: 'text', text: 'in alpha', tone: 'neutral' }] },
        { id: 'b', label: 'Beta', children: [{ type: 'text', text: 'in beta', tone: 'neutral' }] },
      ],
    });
    expect(screen.getByText('in alpha')).toBeTruthy();
    expect(screen.queryByText('in beta')).toBeNull();
    await userEvent.click(screen.getByRole('tab', { name: 'Beta' }));
    expect(screen.getByText('in beta')).toBeTruthy();
  });

  it('sends a plain button action at once', async () => {
    const run = show(button());
    await userEvent.click(screen.getByRole('button', { name: 'Reset' }));
    expect(run).toHaveBeenCalledWith({ id: 'reset', payload: null });
  });

  it('holds a confirming button until the user agrees', async () => {
    const run = show(button('Reset every counter?'));
    await userEvent.click(screen.getByRole('button', { name: 'Reset' }));
    expect(await screen.findByText('Reset every counter?')).toBeTruthy();
    expect(run).not.toHaveBeenCalled();

    await userEvent.click(
      screen.getByRole('button', { name: m['app.action.cancel']() }),
    );
    expect(run).not.toHaveBeenCalled();

    await userEvent.click(screen.getByRole('button', { name: 'Reset' }));
    await userEvent.click(
      await screen.findByRole('button', { name: m['app.action.confirm']() }),
    );
    expect(run).toHaveBeenCalledWith({ id: 'reset', payload: null });
  });

  it('filters table rows by what is typed', async () => {
    show({
      type: 'table',
      columns: ['Name'],
      filter: true,
      rows: [
        { cells: ['hestia'], payload: null },
        { cells: ['kodo'], payload: null },
      ],
    });
    expect(screen.getByText('kodo')).toBeTruthy();
    await userEvent.type(
      screen.getByRole('searchbox', { name: m['plugins.filter']() }),
      'HES',
    );
    expect(screen.queryByText('kodo')).toBeNull();
    expect(screen.getByText('hestia')).toBeTruthy();
  });

  it('has no filter box unless the plugin asked for one', () => {
    show({
      type: 'table',
      columns: ['Name'],
      filter: false,
      rows: [{ cells: ['hestia'], payload: null }],
    });
    expect(screen.queryByRole('searchbox')).toBeNull();
  });

  it('draws a remote image from the daemon, never from the host', async () => {
    show({
      type: 'remote_image',
      url: 'https://mc-heads.net/avatar/hestia',
      alt: 'hestia',
      shape: 'avatar',
    });
    await waitFor(() =>
      expect(
        document.querySelector('img[alt="hestia"]')?.getAttribute('src'),
      ).toMatch(/^data:image\/png;base64,/),
    );
    expect(document.querySelector('img[src^="http"]')).toBeNull();
  });
});

describe('a live page', () => {
  it('draws again when a topic it names is published, and only then', async () => {
    installFakeDaemon();
    const original = channels['plugin.ui.render'];
    const drawn = vi.fn(original);
    channels['plugin.ui.render'] = drawn;
    try {
      renderWithProviders(<PluginView plugin="hello" page="greeter" />, {
        route: true,
      });
      await waitFor(() => expect(drawn).toHaveBeenCalledTimes(1));
      await waitFor(() =>
        expect(screen.getByText('Online')).toBeTruthy(),
      );

      await act(async () => {
        publish('process.exit', { id: 'x' } as never);
        await new Promise((resolve) => setTimeout(resolve, 700));
      });
      expect(drawn).toHaveBeenCalledTimes(1);

      await act(async () => {
        publish('server.ready', { id: 'srv' } as never);
        await new Promise((resolve) => setTimeout(resolve, 700));
      });
      await waitFor(() => expect(drawn).toHaveBeenCalledTimes(2));
    } finally {
      channels['plugin.ui.render'] = original;
    }
  });
});
