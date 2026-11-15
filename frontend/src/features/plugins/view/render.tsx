import type { UiNode } from '@/api/types/ui';
import { cn } from '@/lib/utils';
import { m } from '@/paraglide/messages.js';

import { ActionButton, Detail, List, Table } from './data';
import { UiForm } from './form';
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
          {node.children.map((child, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: a tree is positional
            <UiView key={i} node={child} />
          ))}
        </div>
      );
    case 'section':
      return (
        <div className="flex flex-col gap-4">
          <Heading text={node.title} level={2} />
          <UiView
            node={{
              type: 'stack',
              direction: 'vertical',
              children: [...node.actions, ...node.children],
            }}
          />
        </div>
      );
    case 'copy':
      return <Text text={node.text} tone="neutral" />;
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
