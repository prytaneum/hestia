import type { UiNode } from '@/api/types/ui';
import { Button } from '@/components/ui/button';

import { PluginIcon } from '../icon';
import { useView } from './context';
import { UiView } from './render';

type Node<T extends UiNode['type']> = Extract<UiNode, { type: T }>;

export function Table({ columns, rows, rowAction }: Node<'table'>) {
  const { run, pending } = useView();
  return (
    <div className="max-w-full overflow-x-auto border border-border">
      <table className="w-full text-left text-xs">
        <thead className="border-b border-border bg-muted/40 text-muted-foreground">
          <tr>
            {columns.map((column, i) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: columns carry no id
              <th key={i} className="px-3 py-2 font-medium">
                {column}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-border">
          {rows.map((row, r) => (
            <tr
              // biome-ignore lint/suspicious/noArrayIndexKey: rows carry no id
              key={r}
              className={rowAction ? 'cursor-pointer hover:bg-muted/50' : ''}
              aria-disabled={pending || undefined}
              onClick={
                rowAction
                  ? () => void run({ id: rowAction, payload: row.payload })
                  : undefined
              }
            >
              {row.cells.map((cell, c) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: cells carry no id
                <td key={c} className="px-3 py-2">
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function List({ items }: Node<'list'>) {
  const { plugin } = useView();
  return (
    <div className="divide-y divide-border border border-border">
      {items.map((item, i) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: items carry no id
          key={i}
          className="flex min-h-11 items-center gap-3 px-3 py-2"
        >
          {item.icon && (
            <PluginIcon
              plugin={plugin}
              icon={item.icon}
              className="size-4 shrink-0 text-muted-foreground"
            />
          )}
          <div className="min-w-0 flex-1">
            <p className="truncate text-xs">{item.title}</p>
            {item.subtitle && (
              <p className="truncate text-[11px] text-muted-foreground">
                {item.subtitle}
              </p>
            )}
          </div>
          <div className="flex items-center gap-2">
            {item.actions.map((action, a) => (
              // biome-ignore lint/suspicious/noArrayIndexKey: actions carry no id
              <UiView key={a} node={action} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

export function Detail({ pairs }: Node<'detail'>) {
  return (
    <div className="divide-y divide-border border border-border px-3">
      {pairs.map((pair, i) => (
        <div
          // biome-ignore lint/suspicious/noArrayIndexKey: pairs carry no id
          key={i}
          className="flex items-baseline justify-between gap-4 py-1.5"
        >
          <span className="text-xs text-muted-foreground">{pair.label}</span>
          <span className="text-right font-mono text-xs">{pair.value}</span>
        </div>
      ))}
    </div>
  );
}

const VARIANT = {
  primary: 'default',
  secondary: 'outline',
  danger: 'destructive',
} as const;

export function ActionButton({ label, action, variant }: Node<'button'>) {
  const { run, pending } = useView();
  return (
    <Button
      variant={VARIANT[variant]}
      disabled={pending}
      onClick={() => void run(action)}
    >
      {label}
    </Button>
  );
}
