export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

export function parentOf(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut > 0 ? path.slice(0, cut) : path;
}
