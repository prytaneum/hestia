import type { IconWeight } from '@phosphor-icons/react';

import { namedIcon } from '@/components/icons';
import { pluginAsset } from '@/lib/plugin-asset';
import { cn } from '@/lib/utils';

import { isIconPath } from './lib';

/** A page's icon: a name from the app's icon set, or an image in the plugin's package. */
export function PluginIcon({
  plugin,
  icon,
  weight,
  className,
}: {
  plugin: string;
  icon?: string;
  weight?: IconWeight;
  className?: string;
}) {
  if (icon && isIconPath(icon)) {
    return (
      <img
        src={pluginAsset(plugin, icon)}
        alt=""
        className={cn('object-contain', className)}
      />
    );
  }
  const Glyph = namedIcon(icon);
  return <Glyph weight={weight} className={className} />;
}
