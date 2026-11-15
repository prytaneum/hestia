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
import { jobIdOf, runPlan } from '@/mock/job';
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

describe('a page action answering run', () => {
  it('starts the command for the view plugin and entry, whatever the effect carries', async () => {
    installFakeDaemon();
    const originals = {
      render: channels['plugin.ui.render'],
      action: channels['plugin.ui.action'],
      run: channels['plugin.run'],
    };
    const started = vi.fn((p: Record<string, unknown>) =>
      runPlan({
        id: jobIdOf(p, 'plugin-run'),
        channel: 'plugin.run',
        ticks: [],
        done: () => ({ output: null }),
      }),
    );
    channels['plugin.ui.render'] = () => button();
    channels['plugin.ui.action'] = () => [
      {
        type: 'run',
        command: 'setup',
        args: { port: 1 },
        plugin: 'evil',
        entry: 'other',
      },
    ];
    channels['plugin.run'] = started;
    try {
      renderWithProviders(
        <PluginView plugin="hello" page="greeter" entry="srv" kind="server" />,
        { route: true },
      );
      await userEvent.click(
        await screen.findByRole('button', { name: 'Reset' }),
      );
      await waitFor(() => expect(started).toHaveBeenCalledTimes(1));
      expect(started.mock.calls[0][0]).toMatchObject({
        plugin: 'hello',
        command: 'setup',
        entry: 'srv',
        args: { port: 1 },
      });
    } finally {
      channels['plugin.ui.render'] = originals.render;
      channels['plugin.ui.action'] = originals.action;
      channels['plugin.run'] = originals.run;
    }
  });
});

describe('a form with a number field', () => {
  const form = (): UiNode =>
    ({
      type: 'form',
      id: 'port',
      fields: [
        { type: 'number', name: 'port', label: 'Port', value: 80, min: 1, max: 100 },
      ],
      submit: { label: 'Save', action: { id: 'save', payload: null } },
    }) as UiNode;

  it('does not submit while the number is empty or out of range', async () => {
    const run = show(form());
    const save = screen.getByRole('button', { name: 'Save' });
    const input = screen.getByLabelText('Port');

    await userEvent.clear(input);
    expect((save as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(save);

    await userEvent.type(input, '500');
    expect((save as HTMLButtonElement).disabled).toBe(true);

    await userEvent.clear(input);
    await userEvent.type(input, '25');
    await userEvent.click(save);
    expect(run).toHaveBeenCalledTimes(1);
    expect(run).toHaveBeenCalledWith({ id: 'save', payload: null }, { port: 25 });
  });
});
