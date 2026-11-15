import { screen } from '@testing-library/react';
import { userEvent } from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import type { ContentSource } from '@/api';
import { FilterMenu } from '@/components/filter-menu';
import { sourceGroup } from '@/features/content/components';
import { m } from '@/paraglide/messages.js';
import { renderWithProviders, resetQueryCache } from '../../../support';

const ready: ContentSource = {
  id: 'modrinth',
  name: 'Modrinth',
  kinds: ['mod'],
  editions: ['java'],
};
const waitingOn = (
  id: string,
  setting: { plugin: string; key: string },
): ContentSource => ({
  id,
  name: id,
  kinds: ['mod'],
  editions: ['java'],
  requiresSetting: setting,
});

afterEach(resetQueryCache);

describe('a source waiting on a setting', () => {
  it('links to the plugin setting it needs', async () => {
    renderWithProviders(
      <FilterMenu
        groups={[
          sourceGroup([ready], '', () => {}, [
            waitingOn('static.index', { plugin: 'static', key: 'api-key' }),
          ]),
        ]}
      />,
      { route: true },
    );
    await userEvent.click(
      await screen.findByRole('button', { name: m['app.collection.filter']() }),
    );

    const plugin = await screen.findByRole('menuitem', { name: /api-key/ });
    expect(plugin.getAttribute('href')).toBe(
      '/settings?tab=plugins&plugin=static&field=static-api-key',
    );
  });
});
