import { useEffect, useRef } from 'react';

import { errorMessage } from '@/api';
import type { UiNode } from '@/api/types/ui';
import { pluginAsset } from '@/lib/plugin-asset';
import { m } from '@/paraglide/messages.js';
import { useJobMutation } from '@/queries/jobs';
import { pluginMutations } from '@/queries/plugin';

import {
  type BridgeReply,
  type BridgeRequest,
  parseBridgeMessage,
} from './bridge';
import { useView } from './context';

type Node<T extends UiNode['type']> = Extract<UiNode, { type: T }>;

/**
 * A plugin's own page in a sandboxed frame. The sandbox grants scripts only —
 * never `allow-same-origin` — so the frame has an opaque origin and no reach
 * into the app or its IPC. Its only channel is `postMessage`, answered here for
 * this plugin and no other.
 */
export function Webview({ src, height }: Node<'webview'>) {
  const { plugin, run } = useView();
  const frame = useRef<HTMLIFrameElement>(null);
  const command = useJobMutation(pluginMutations.run());
  const runCommand = command.mutateAsync;

  useEffect(() => {
    const serve = async (request: BridgeRequest): Promise<BridgeReply> => {
      try {
        if (request.type === 'run') {
          const done = await runCommand({
            plugin,
            command: request.command,
            title: request.command,
            args: request.args,
          });
          return { id: request.id, ok: true, result: done.output };
        }
        await run({ id: request.action, payload: request.payload });
        return { id: request.id, ok: true, result: null };
      } catch (error) {
        return { id: request.id, ok: false, error: errorMessage(error) };
      }
    };

    const onMessage = (event: MessageEvent) => {
      const target = frame.current?.contentWindow;
      if (!target || event.source !== target) return;
      const request = parseBridgeMessage(event.data);
      if (!request) return;
      void serve(request).then((reply) => {
        // An opaque-origin frame can only be addressed with '*'; the reply goes to the window this component created.
        frame.current?.contentWindow?.postMessage(reply, '*');
      });
    };
    window.addEventListener('message', onMessage);
    return () => window.removeEventListener('message', onMessage);
  }, [plugin, run, runCommand]);

  return (
    <iframe
      ref={frame}
      title={m['plugins.webview_title']({ plugin })}
      src={pluginAsset(plugin, src)}
      sandbox="allow-scripts"
      allow=""
      referrerPolicy="no-referrer"
      style={{ height }}
      className="w-full border border-border bg-background"
    />
  );
}
