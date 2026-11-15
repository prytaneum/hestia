import type { ServerPort } from '@/api';

/** The one place a server's ports are turned into text: what a player forwards. */
export const formatPorts = (ports: ServerPort[]): string =>
  ports.map(({ port, protocol }) => `${port}/${protocol}`).join(', ');
