/**
 * The dispatch seam. Every `invoke()` the frontend makes arrives here: the
 * generic `ipc_call` bridge is routed by its channel, everything else by its
 * command name — the same split the desktop shell makes.
 */
import { channels } from './channels';
import { commands } from './commands';

/**
 * Safety net for an unlisted channel: an empty array-backed proxy whose every
 * property is another such proxy. It is `Array.isArray`-true (a valid, empty
 * React child and mappable) and never yields `undefined`, so an unmocked read
 * degrades to "empty" instead of crashing a query or a `.length`/`.x` access.
 */
function emptyProxy(): unknown {
  return new Proxy([] as unknown[], {
    get(target, prop, receiver) {
      if (prop in target || typeof prop === 'symbol') {
        return Reflect.get(target, prop, receiver);
      }
      return emptyProxy();
    },
  });
}

function hasFixture(channel: string): channel is keyof typeof channels {
  return Object.hasOwn(channels, channel);
}

export function dispatch(cmd: string, args: Record<string, unknown>): unknown {
  if (cmd === 'ipc_call') {
    const channel = String(args.channel);
    const handler = hasFixture(channel) ? channels[channel] : undefined;
    if (handler)
      return handler((args.payload ?? {}) as Record<string, unknown>);
    console.warn(
      `[mock] no fixture for channel "${channel}" — returning empty`,
    );
    return emptyProxy();
  }

  const command = commands[cmd];
  if (command) return command(args);
  console.warn(`[mock] no fixture for command "${cmd}" — returning null`);
  return null;
}
