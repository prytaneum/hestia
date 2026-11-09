/** The one place a server's ports are turned into text. */
export const formatPorts = (ports: number[]): string => ports.join(', ');
