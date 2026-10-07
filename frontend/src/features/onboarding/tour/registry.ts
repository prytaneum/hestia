import { m } from '@/paraglide/messages.js';

import type { TourAnchor } from './anchor';

export type TourId = 'shell' | 'browse' | 'instance' | 'server' | 'skins';

export interface TourStep {
  anchor?: TourAnchor;
  title: () => string;
  body: () => string;
}

export const tours: Record<TourId, readonly TourStep[]> = {
  shell: [
    {
      anchor: 'nav',
      title: m['onboarding.tour.shell.nav.title'],
      body: m['onboarding.tour.shell.nav.body'],
    },
    {
      anchor: 'library-new',
      title: m['onboarding.tour.shell.create.title'],
      body: m['onboarding.tour.shell.create.body'],
    },
    {
      anchor: 'play-bar',
      title: m['onboarding.tour.shell.play.title'],
      body: m['onboarding.tour.shell.play.body'],
    },
    {
      anchor: 'account',
      title: m['onboarding.tour.shell.account.title'],
      body: m['onboarding.tour.shell.account.body'],
    },
    {
      anchor: 'daemon-status',
      title: m['onboarding.tour.shell.daemon.title'],
      body: m['onboarding.tour.shell.daemon.body'],
    },
  ],

  browse: [
    {
      anchor: 'page-search',
      title: m['onboarding.tour.browse.search.title'],
      body: m['onboarding.tour.browse.search.body'],
    },
    {
      anchor: 'browse-kinds',
      title: m['onboarding.tour.browse.filters.title'],
      body: m['onboarding.tour.browse.filters.body'],
    },
  ],

  instance: [
    {
      anchor: 'entry-run',
      title: m['onboarding.tour.instance.play.title'],
      body: m['onboarding.tour.instance.play.body'],
    },
    {
      anchor: 'instance-content',
      title: m['onboarding.tour.instance.content.title'],
      body: m['onboarding.tour.instance.content.body'],
    },
    {
      anchor: 'instance-worlds',
      title: m['onboarding.tour.instance.worlds.title'],
      body: m['onboarding.tour.instance.worlds.body'],
    },
  ],

  server: [
    {
      anchor: 'entry-run',
      title: m['onboarding.tour.server.start.title'],
      body: m['onboarding.tour.server.start.body'],
    },
    {
      anchor: 'server-details',
      title: m['onboarding.tour.server.address.title'],
      body: m['onboarding.tour.server.address.body'],
    },
    {
      anchor: 'server-console',
      title: m['onboarding.tour.server.console.title'],
      body: m['onboarding.tour.server.console.body'],
    },
    {
      anchor: 'server-backups',
      title: m['onboarding.tour.server.backups.title'],
      body: m['onboarding.tour.server.backups.body'],
    },
  ],

  skins: [
    {
      anchor: 'skin-preview',
      title: m['onboarding.tour.skins.preview.title'],
      body: m['onboarding.tour.skins.preview.body'],
    },
    {
      anchor: 'skin-library',
      title: m['onboarding.tour.skins.library.title'],
      body: m['onboarding.tour.skins.library.body'],
    },
    {
      anchor: 'page-actions',
      title: m['onboarding.tour.skins.add.title'],
      body: m['onboarding.tour.skins.add.body'],
    },
  ],
};
