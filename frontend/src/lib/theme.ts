const LINK_ID = 'plugin-theme';

/**
 * Loads a plugin stylesheet after the app's own, so equal-specificity rules in
 * it win. A `light` theme also drops the `dark` class the app always carries.
 * Returns the undo; `onError` runs if the stylesheet fails to load.
 */
export function applyTheme(
  href: string,
  light: boolean,
  onError: () => void,
): () => void {
  const root = document.documentElement;
  const link = document.createElement('link');
  link.id = LINK_ID;
  link.rel = 'stylesheet';
  link.addEventListener('error', onError, { once: true });
  link.href = href;
  document.head.append(link);
  if (light) root.classList.remove('dark');

  return () => {
    link.remove();
    root.classList.add('dark');
  };
}
