export type TourAnchor =
  | 'nav'
  | 'daemon-status'
  | 'play-bar'
  | 'page-search'
  | 'page-actions'
  | 'browse-kinds'
  | 'entry-run'
  | 'instance-content'
  | 'instance-worlds'
  | 'server-details'
  | 'server-console'
  | 'server-backups'
  | 'skin-preview'
  | 'skin-library';

const ATTRIBUTE = 'data-tour';

export function anchor(id: TourAnchor): Record<string, string> {
  return { [ATTRIBUTE]: id };
}

export function findAnchor(id: TourAnchor): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[${ATTRIBUTE}="${id}"]`);
}
