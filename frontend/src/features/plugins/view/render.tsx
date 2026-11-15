import type { UiNode } from '@/api/types/ui';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

import { CopyValue } from './copy';
import { ActionButton, Detail, List, Table } from './data';
import { UiForm } from './form';
import { keyed } from './keys';
import {
  Heading,
  Image,
  Markup,
  Meter,
  Notice,
  PluginBadge,
  RemoteImage,
  Text,
} from './layout';
import { UiTabs } from './tabs';
import { Webview } from './webview';

/** Draws one node of a plugin tree with the app's own components; a node it does not know is shown as a notice. */
export function UiView({ node }: { node: UiNode }) {
  switch (node.type) {
    case 'stack':
      return (
        <div
          className={cn(
            'flex gap-4',
            node.direction === 'horizontal'
              ? 'flex-row flex-wrap items-center'
              : 'flex-col',
          )}
        >
          {keyed(node.children).map(({ key, item }) => (
            <UiView key={key} node={item} />
          ))}
        </div>
      );
    case 'section':
      return (
        <section className="flex flex-col gap-4 border border-border p-4">
          <div className="flex items-start justify-between gap-4">
            <div className="min-w-0 space-y-1">
              <Heading text={node.title} level={2} />
              {node.description && (
                <Text text={node.description} tone="muted" />
              )}
            </div>
            <div className="flex shrink-0 items-center gap-2">
              {keyed(node.actions).map(({ key, item }) => (
                <UiView key={key} node={item} />
              ))}
            </div>
          </div>
          {keyed(node.children).map(({ key, item }) => (
            <UiView key={key} node={item} />
          ))}
        </section>
      );
    case 'copy':
      return <CopyValue text={node.text} label={node.label} />;
    case 'heading':
      return <Heading text={node.text} level={node.level} />;
    case 'text':
      return <Text text={node.text} tone={node.tone} />;
    case 'markdown':
      return <Markup source={node.source} />;
    case 'image':
      return <Image src={node.src} alt={node.alt} />;
    case 'remote_image':
      return <RemoteImage url={node.url} alt={node.alt} shape={node.shape} />;
    case 'badge':
      return <PluginBadge text={node.text} tone={node.tone} />;
    case 'tabs':
      return <UiTabs {...node} />;
    case 'table':
      return <Table {...node} />;
    case 'list':
      return <List {...node} />;
    case 'detail':
      return <Detail {...node} />;
    case 'form':
      return <UiForm key={node.id} {...node} />;
    case 'button':
      return <ActionButton {...node} />;
    case 'notice':
      return <Notice text={node.text} tone={node.tone} />;
    case 'progress':
      return <Meter fraction={node.fraction} label={node.label} />;
    case 'webview':
      return <Webview {...node} />;
    default:
      return (
        <Notice
          tone="warning"
          text={m['plugins.unknown_node']({
            type: String((node as { type?: unknown }).type),
          })}
        />
      );
  }
}
